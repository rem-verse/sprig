//! Utilities for building a client to communicate over the TCP layer 4
//! protocol.
//!
//! Note this "TCP Client" class allows connecting to multiple 'servers' at
//! once, as well as "binding" to listen as a 'host'. This is mostly due to the
//! fact we have to support SDIO where the person who "connect"'s acts as
//! the server.
//!
//! ## TCP and NAGLE'ing
//!
//! TCP the protocol is fundamentally built on-top of a 'stream', and is
//! not packet oriented. For a client this affects us in two places:
//!
//! 1. When reading responses back. Similar to a server we have to know when
//!    a packet starts/ends in the response. We use the same nagle guard method
//!    that our servers use.
//! 2. When writing our requests, just because our servers are well behaved,
//!    this doesn't mean that all server implementations are well behaved. So,
//!    we will automatically turn on `no_delay` to prevent nagle's algorithm
//!    from getting in our way. So our write calls will always lead to
//!    hopefully one write call on the other side.
//!
//! ## API Notes
//!
//! Most, TCP Clients only expect to connect to a single server, and have just
//! a single TCP Stream. This means just one client to `send`, and `recv` from.
//! HOWEVER, this simple model doesn't work for two reasons:
//!
//! 1. SDIO's TCP "Client" is actually a TCP Server that allows multiple folks
//!    to connect to it. Although in reality folks are only supposed to connect
//!    once, it techincally supports multiple.
//! 2. Scientists. One goal of all of our clients/servers is they allow
//!    debugging with scientists. Scientists allow connecting to many upstreams
//!    at once, and diff'ing between them. So our TCP Client needs to fit in
//!    there as well.
//!
//! In order to keep the APIs as simple as possible (keeping one "send", and
//! one "recv" interface) while also supporting multiple clients that need to
//! broadcast to all we introduce the concept of a "primary" connection to
//! treat responses as "canonical" from. By default this is whoever we connect
//! to, or whoever connects to us first. You can always re-assign this
//! manually, but the first is always "primary". If the primary connection is
//! dropped, we will switch to the next oldest connection automatically.
//!
//! The way this works is when you call `send` the response you get will always
//! be from this "primary" connection, all the rest will be silently dropped,
//! but will still have sent the packet (and read the response) from all other
//! connections. This way when dealing with stateful protocols, it all will
//! continue to work.
//!
//! ### Scientists
//!
//! Scientists on the other hand can call the [`TCPClient::should_keep_all_responses`]
//! API, which will allow clients to act the same, but instead of dropping the
//! responses from other upstreams will keep them until they can be removed
//! later with
//! ([`TCPClient::take_all_response_for_request_id`]).

use crate::{
	errors::{CatBridgeError, NetworkError},
	net::{
		DEFAULT_SLOWLORIS_TIMEOUT, STREAM_ID, TCP_READ_BUFFER_SIZE,
		additions::RequestID,
		client::{
			errors::CommonNetClientNetworkError,
			models::{
				DisconnectAsyncDropClient, RequestStreamEvent, RequestStreamMessage,
				UnderlyingOnStreamBeginService, UnderlyingOnStreamEndService,
			},
		},
		errors::{CommonNetAPIError, CommonNetNetworkError},
		handlers::{
			OnRequestStreamBeginHandler, OnRequestStreamEndHandler, OnStreamBeginHandlerAsService,
			OnStreamEndHandlerAsService,
		},
		models::{FromRequestParts, NagleGuard, PostNagleFnTy, PreNagleFnTy, Request, Response},
		now,
	},
};
use bytes::{Bytes, BytesMut};
use fnv::{FnvHashMap, FnvHashSet};
use futures::future::join_all;
use miette::miette;
use scc::HashMap as ConcurrentHashMap;
use std::{
	fmt::{Debug, Formatter, Result as FmtResult},
	hash::BuildHasherDefault,
	net::{Ipv4Addr, SocketAddr, SocketAddrV4},
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::{Duration, Instant, SystemTime},
};
use tokio::{
	io::{AsyncReadExt, AsyncWriteExt},
	net::{TcpListener, TcpStream, ToSocketAddrs},
	sync::mpsc::{
		Receiver as BoundedReceiver, Sender as BoundedSender, channel as bounded_channel,
		error::SendTimeoutError,
	},
	task::{Builder as TaskBuilder, block_in_place},
	time::{sleep, timeout},
};
use tower::{Layer, Service, util::BoxCloneService};
use tracing::{Instrument, debug, error_span, trace, warn};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

#[cfg(debug_assertions)]
use crate::net::SPRIG_TRACE_IO;

const EMPTY_TIMEOUT: Duration = Duration::from_secs(0);

/// A generic TCP client that can handle connections to multiple TCP Servers.
///
/// This client allows for connecting to many servers all at once, or creating
/// a real "TCP Server" that allows people to connect to it (for SDIO). Because
/// of the weirdness of our SDIO client, and the hooks for scientists this
/// client ends up accepting a lot of parameters, see the module documentation
/// for more information.
pub struct TCPClient {
	/// Cat-dev's need load-bearing sleeps as they can "ACK" a ppacket, but
	/// throw away the bytes and pretend it never got implied.
	///
	/// This is used for sleeping before then. By default this isn't set,
	/// cat-dev services will call: [`TCPClient::set_cat_dev_slowdown`].
	cat_dev_slowdown: Option<Duration>,
	/// For devices that can't receive too much data at once.
	///
	/// When devices *cough* MION *cough* can't receive too much data at once,
	/// we need to chunk it, and rest between those chunks. This will chunk those
	/// packets for us.
	chunk_output_at_size: Option<usize>,
	/// Keep responses for all streams, mostly used for scientist like code.
	keep_all_responses: bool,
	/// Determines when a packet starts, and ends.
	nagle_guard: NagleGuard,
	/// A tower service to call when a particular stream starts.
	///
	/// This is effectively like an "on connect" hook that you can use to
	/// call functions.
	on_stream_begin: Option<UnderlyingOnStreamBeginService<()>>,
	/// A tower service to call when a particular stream ends.
	///
	/// This is effectively like an "on disconnect" hook that you can use
	/// to call functions.
	on_stream_end: Option<UnderlyingOnStreamEndService<()>>,
	/// A function to apply some sort of processing before doing any sort
	/// of NAGLE/SLOWLORIS logic.
	///
	/// This is best used for encryption/decryption that needs to be handled
	/// before anything else.
	pre_nagle_hook: Option<&'static dyn PreNagleFnTy>,
	/// A function to apply some sort of processing before sending a packet
	/// out.
	///
	/// This is the very last thing before `send` is actually called. This is
	/// best used for encryption/decryption that needs to be only processed
	/// at the end.
	post_nagle_hook: Option<&'static dyn PostNagleFnTy>,
	/// The stream id of the current "active" stream.
	primary_stream_id: Arc<AtomicU64>,
	/// The list of active client streams.
	streams: Arc<ConcurrentHashMap<u64, TCPClientStream>>,
	/// The name of the service being provided by this server, to attach to logs.
	service_name: &'static str,
	/// The "Slowloris Detection" timeout.
	slowloris_timeout: Duration,
	/// If we should log all packet requests/responses when compiled with debug
	/// assertions.
	#[cfg(debug_assertions)]
	trace_during_debug: bool,
}

impl TCPClient {
	/// Construct a new TCP Client.
	///
	/// This TCP client will by default not connect to anything/listen to any
	/// ports. Instead you will have to call [`TCPClient::connect`], and
	/// [`TCPClient::bind`] manually to setup connections.
	///
	/// Remember the default [`TCPClient::send`], and [`TCPClient::receive`] will
	/// only return responses to the "primary" connection (whichever connection
	/// has the oldest connection). That is unless you call
	/// [`TCPClient::set_primary_stream`].
	#[must_use]
	pub fn new(
		service_name: &'static str,
		guard: impl Into<NagleGuard>,
		nagle_hooks: (
			Option<&'static dyn PreNagleFnTy>,
			Option<&'static dyn PostNagleFnTy>,
		),
		trace_io_during_debug: bool,
	) -> Self {
		#[cfg(not(debug_assertions))]
		{
			if trace_io_during_debug {
				warn!(
					"Trace IO was turned on, but debug assertsions were not compiled in. Tracing of I/O will not happen. Please recompile cat-dev with debug assertions to properly trace I/O.",
				);
			}
		}

		Self {
			cat_dev_slowdown: None,
			chunk_output_at_size: None,
			keep_all_responses: false,
			nagle_guard: guard.into(),
			on_stream_begin: None,
			on_stream_end: None,
			pre_nagle_hook: nagle_hooks.0,
			post_nagle_hook: nagle_hooks.1,
			primary_stream_id: Arc::new(AtomicU64::new(0)),
			service_name,
			slowloris_timeout: DEFAULT_SLOWLORIS_TIMEOUT,
			streams: Arc::new(ConcurrentHashMap::default()),
			#[cfg(debug_assertions)]
			trace_during_debug: trace_io_during_debug || *SPRIG_TRACE_IO,
		}
	}

	/// Set the slowdown to before sending bytes from this server.
	pub const fn set_cat_dev_slowdown(&mut self, slowdown: Option<Duration>) {
		self.cat_dev_slowdown = slowdown;
	}

	/// Mark that this client should keep all responses (e.g. those from the non
	/// active upstreams).
	pub const fn should_keep_all_responses(&mut self) {
		self.keep_all_responses = true;
	}

	/// Set directly whether or not this client should keep all responses.
	pub const fn set_keep_all_responses(&mut self, keep: bool) {
		self.keep_all_responses = keep;
	}

	/// Set the primary stream to receive responses from.
	pub fn set_primary_stream(&mut self, stream_id: u64) {
		self.primary_stream_id.store(stream_id, Ordering::Release);
	}

	#[must_use]
	pub const fn chunk_output_at_size(&self) -> Option<usize> {
		self.chunk_output_at_size
	}

	pub const fn set_chunk_output_at_size(&mut self, new_size: Option<usize>) {
		self.chunk_output_at_size = new_size;
	}

	#[must_use]
	pub const fn slowloris_timeout(&self) -> Duration {
		self.slowloris_timeout
	}
	pub const fn set_slowloris_timeout(&mut self, slowloris_timeout: Duration) {
		self.slowloris_timeout = slowloris_timeout;
	}

	#[must_use]
	pub const fn on_stream_begin(&self) -> Option<&UnderlyingOnStreamBeginService<()>> {
		self.on_stream_begin.as_ref()
	}

	/// Set a hook to run when a stream has been created.
	///
	/// This is what happens when a new machine connects. So it may also be
	/// refered to as "on connect". This assumes you're assigning a service
	/// that already exists and is in the raw storage type, you may want to
	/// look into [`Self::set_on_stream_begin`], or
	/// [`Self::set_on_stream_begin_service`].
	///
	/// ## Errors
	///
	/// If the stream beginning hook has already been registered. If you're
	/// looking to perform multiple actions at once, look into layer-ing.
	pub fn set_raw_on_stream_begin(
		&mut self,
		on_start: Option<UnderlyingOnStreamBeginService<()>>,
	) -> Result<(), CommonNetAPIError> {
		if self.on_stream_begin.is_some() {
			return Err(CommonNetAPIError::OnStreamBeginAlreadyRegistered);
		}

		self.on_stream_begin = on_start;
		Ok(())
	}

	/// Set a function hook to run when a stream has been created.
	///
	/// This is what happens when a new machine connects. So it may also be
	/// refered to as "on connect". This assumes you're assigning a function
	/// to on stream begin otherwise use [`Self::set_raw_on_stream_begin`],
	/// or [`Self::set_on_stream_begin_service`].
	///
	/// ## Errors
	///
	/// If the stream beginning hook has already been registered. If you're
	/// looking to perform multiple actions at once, look into layer-ing.
	pub fn set_on_stream_begin<HandlerTy, HandlerParamsTy>(
		&mut self,
		handler: HandlerTy,
	) -> Result<(), CommonNetAPIError>
	where
		HandlerParamsTy: Send + 'static,
		HandlerTy: OnRequestStreamBeginHandler<HandlerParamsTy, ()> + Clone + Send + 'static,
	{
		if self.on_stream_begin.is_some() {
			return Err(CommonNetAPIError::OnStreamBeginAlreadyRegistered);
		}

		let boxed = BoxCloneService::new(OnStreamBeginHandlerAsService::new(handler));
		self.on_stream_begin = Some(boxed);
		Ok(())
	}

	/// Set a function hook to run when a stream has been created.
	///
	/// This is what happens when a new machine connects. So it may also be
	/// refered to as "on connect". This assumes you're assigning a [`Service`]
	/// to on stream begin otherwise use [`Self::set_raw_on_stream_begin`],
	/// or [`Self::set_on_stream_begin`].
	///
	/// ## Errors
	///
	/// If the stream beginning hook has already been registered. If you're
	/// looking to perform multiple actions at once, look into layer-ing.
	pub fn set_on_stream_begin_service<ServiceTy>(
		&mut self,
		service_ty: ServiceTy,
	) -> Result<(), CommonNetAPIError>
	where
		ServiceTy: Clone
			+ Send
			+ Service<RequestStreamEvent<()>, Response = bool, Error = CatBridgeError>
			+ 'static,
		ServiceTy::Future: Send + 'static,
	{
		if self.on_stream_begin.is_some() {
			return Err(CommonNetAPIError::OnStreamBeginAlreadyRegistered);
		}

		self.on_stream_begin = Some(BoxCloneService::new(service_ty));
		Ok(())
	}

	/// Add a layer to the service to process when a stream begins, or a new
	/// connection is created.
	///
	/// ## Errors
	///
	/// If there is no on stream begin handler that is currently active.
	pub fn layer_on_stream_begin<LayerTy, ServiceTy>(
		&mut self,
		layer: LayerTy,
	) -> Result<(), CommonNetAPIError>
	where
		LayerTy: Layer<UnderlyingOnStreamBeginService<()>, Service = ServiceTy>,
		ServiceTy: Service<RequestStreamEvent<()>, Response = bool, Error = CatBridgeError>
			+ Clone
			+ Send
			+ 'static,
		<LayerTy::Service as Service<RequestStreamEvent<()>>>::Future: Send + 'static,
	{
		let Some(srvc) = self.on_stream_begin.take() else {
			return Err(CommonNetAPIError::OnStreamBeginNotRegistered);
		};

		self.on_stream_begin = Some(BoxCloneService::new(layer.layer(srvc)));
		Ok(())
	}

	#[must_use]
	pub const fn on_stream_end(&self) -> Option<&UnderlyingOnStreamEndService<()>> {
		self.on_stream_end.as_ref()
	}

	/// Set a hook to run when a stream is being destroyed.
	///
	/// This is what happens when a machine disconnects. So it may also be
	/// refered to as "on disconnect". This assumes you're assigning a service
	/// that already exists and is in the raw storage type, you may want to
	/// look into [`Self::set_on_stream_end`], or
	/// [`Self::set_on_stream_end_service`].
	///
	/// ## Errors
	///
	/// If the stream ending hook has already been registered. If you're
	/// looking to perform multiple actions at once, look into layer-ing.
	pub fn set_raw_on_stream_end(
		&mut self,
		on_end: Option<UnderlyingOnStreamEndService<()>>,
	) -> Result<(), CommonNetAPIError> {
		if self.on_stream_end.is_some() {
			return Err(CommonNetAPIError::OnStreamEndAlreadyRegistered);
		}

		self.on_stream_end = on_end;
		Ok(())
	}

	/// Set a function hook to run when a stream is being destroyed.
	///
	/// This is what happens when a machine disconnects. So it may also be
	/// refered to as "on disconnect". This assumes you're assigning a function
	/// to on stream end otherwise use [`Self::set_raw_on_stream_end`],
	/// or [`Self::set_on_stream_end_service`].
	///
	/// ## Errors
	///
	/// If the stream ending hook has already been registered. If you're
	/// looking to perform multiple actions at once, look into layer-ing.
	pub fn set_on_stream_end<HandlerTy, HandlerParamsTy>(
		&mut self,
		handler: HandlerTy,
	) -> Result<(), CommonNetAPIError>
	where
		HandlerParamsTy: Send + 'static,
		HandlerTy: OnRequestStreamEndHandler<HandlerParamsTy, ()> + Clone + Send + 'static,
	{
		if self.on_stream_end.is_some() {
			return Err(CommonNetAPIError::OnStreamEndAlreadyRegistered);
		}

		let boxed = BoxCloneService::new(OnStreamEndHandlerAsService::new(handler));
		self.on_stream_end = Some(boxed);
		Ok(())
	}

	/// Set a function hook to run when a stream is being destroyed.
	///
	/// This is what happens when a machine disconnects. So it may also be
	/// refered to as "on disconnect". This assumes you're assigning a [`Service`]
	/// to on stream end otherwise use [`Self::set_raw_on_stream_end`],
	/// or [`Self::set_on_stream_end`].
	///
	/// ## Errors
	///
	/// If the stream beginning hook has already been registered. If you're
	/// looking to perform multiple actions at once, look into layer-ing.
	pub fn set_on_stream_end_service<ServiceTy>(
		&mut self,
		service_ty: ServiceTy,
	) -> Result<(), CommonNetAPIError>
	where
		ServiceTy: Clone
			+ Send
			+ Service<RequestStreamEvent<()>, Response = (), Error = CatBridgeError>
			+ 'static,
		ServiceTy::Future: Send + 'static,
	{
		if self.on_stream_end.is_some() {
			return Err(CommonNetAPIError::OnStreamEndAlreadyRegistered);
		}

		self.on_stream_end = Some(BoxCloneService::new(service_ty));
		Ok(())
	}

	/// Add a layer to the service to process when a stream ends, or a new
	/// connection is destroyed.
	///
	/// ## Errors
	///
	/// If there is no on stream end handler that is currently active.
	pub fn layer_on_stream_end<LayerTy, ServiceTy>(
		&mut self,
		layer: LayerTy,
	) -> Result<(), CommonNetAPIError>
	where
		LayerTy: Layer<UnderlyingOnStreamEndService<()>, Service = ServiceTy>,
		ServiceTy: Service<RequestStreamEvent<()>, Response = (), Error = CatBridgeError>
			+ Clone
			+ Send
			+ 'static,
		<LayerTy::Service as Service<RequestStreamEvent<()>>>::Future: Send + 'static,
	{
		let Some(srvc) = self.on_stream_end.take() else {
			return Err(CommonNetAPIError::OnStreamEndNotRegistered);
		};

		self.on_stream_end = Some(BoxCloneService::new(layer.layer(srvc)));
		Ok(())
	}

	/// "Bind" this client to listen on a specific address.
	///
	/// I know this is unusual for a "TCP Client", binding is for servers? I mean
	/// who exactly would be binding to an address as a client? However, certain
	/// protocols used for PCFS & the like are "reverse". E.g. the pc the server
	/// connects to the "client" the MION listening on a server.
	///
	/// TCP Servers also have a "connect" method for this very reason.
	///
	/// ## Errors
	///
	/// If we cannot spin up a server to listen on this host. This doesn't mean
	/// someone is connected, just that we're listening. You may want to use:
	/// [`TCPClient::wait_for_connection`].
	pub async fn bind<AddrTy: ToSocketAddrs>(&self, address: AddrTy) -> Result<(), CatBridgeError> {
		let listener = TcpListener::bind(address).await.map_err(NetworkError::IO)?;

		let client_address = listener.local_addr().map_err(NetworkError::IO)?;
		let cloned_stream_begin = self.on_stream_begin.clone();
		let cloned_stream_end = self.on_stream_end.clone();
		let cloned_nagle_guard = self.nagle_guard.clone();
		let cloned_slowerloris_timeout = self.slowloris_timeout;
		let streams_ref = self.streams.clone();
		let primary_stream_id_ref = self.primary_stream_id.clone();
		let cloned_chunk_output_at_size = self.chunk_output_at_size;
		let cloned_pre_nagle_hook = self.pre_nagle_hook;
		let cloned_post_nagle_hook = self.post_nagle_hook;
		#[cfg(debug_assertions)]
		let cloned_trace = self.trace_during_debug;
		let cloned_service_name = self.service_name;
		let cloned_cat_dev_slowdown = self.cat_dev_slowdown;

		TaskBuilder::new()
			.name("cat_dev::net::tcp_client::bind().loop")
			.spawn(async move {
				loop {
					let (stream, server_address) = match listener.accept().await {
						Ok(tuple) => tuple,
						Err(cause) => {
							warn!(
								?cause,
								client.address = %client_address,
								"cat_dev::net::tcp_client::bind(): Failed to accept connection!",
							);
							continue;
						}
					};
					let stream_id = STREAM_ID.fetch_add(1, Ordering::SeqCst);

					let cloned_cloned_stream_begin = cloned_stream_begin.clone();
					let cloned_cloned_stream_end = cloned_stream_end.clone();
					let cloned_cloned_nagle_guard = cloned_nagle_guard.clone();
					let cloned_streams_ref = streams_ref.clone();
					let cloned_primary_stream_id_ref = primary_stream_id_ref.clone();

					if let Err(cause) = TaskBuilder::new()
						.name("cat_dev::net::tcp_client::bind().connection.handle")
						.spawn(async move {
							if let Err(cause) = Self::handle_tcp_stream(
								stream,
								stream_id,
								server_address,
								cloned_cloned_stream_begin,
								cloned_cloned_stream_end,
								cloned_cloned_nagle_guard,
								cloned_slowerloris_timeout,
								cloned_streams_ref,
								cloned_primary_stream_id_ref,
								cloned_chunk_output_at_size,
								cloned_pre_nagle_hook,
								cloned_post_nagle_hook,
								cloned_cat_dev_slowdown,
								#[cfg(debug_assertions)]
								cloned_trace,
							)
							.instrument(error_span!(
								"CatDevTCPClientConnect",
								client.address = %client_address,
								server.address = %server_address,
								client.service = cloned_service_name,
								stream.id = stream_id,
								stream.stream_type = "client",
							))
							.await
							{
								warn!(
									?cause,
									client.address = %client_address,
									server.address = %server_address,
									client.service = cloned_service_name,
									"Error escaped while handling TCP Connection.",
								);
							}
						}) {
						warn!(
							?cause,
							client.address = %client_address,
							server.address = %server_address,
							client.service = cloned_service_name,
							"Error handling client connection, no task could be allocated.",
						);
					}

					trace!(
						server.address = %server_address,
						client.address = %client_address,
						"cat_dev::net::tcp_client::bind(): received connection (listener.accept())",
					);
				}
			})
			.map_err(CatBridgeError::SpawnFailure)?;

		Ok(())
	}

	/// This will be an async function that will not return until at least one
	/// server has connected to our client.
	pub async fn wait_for_connection(&self) {
		// Loop until we have an active....
		while self.get_active_sid().await.is_err() {
			sleep(Duration::from_secs(1)).await;
		}
	}

	/// Connect to a new server as a raw TCP client.
	///
	/// This method will return the stream id to use for later requests to
	/// specific streams.
	///
	/// ## Errors
	///
	/// If we can't connect to the remote TCP Server completing the three way
	/// handshake, or if we the stream begin handler returns an error/failure of
	/// some kind.
	pub async fn connect<AddrTy: ToSocketAddrs>(
		&self,
		address: AddrTy,
	) -> Result<u64, CatBridgeError> {
		let raw_stream = TcpStream::connect(address)
			.await
			.map_err(NetworkError::IO)?;
		let stream_id = STREAM_ID.fetch_add(1, Ordering::SeqCst);
		let remote_address = raw_stream.peer_addr().map_err(NetworkError::IO)?;
		let local_address = raw_stream.local_addr().map_err(NetworkError::IO)?;
		trace!(
			server.address = %remote_address,
			client.address = %local_address,
			stream.id = stream_id,
			stream.stream_type = "client",
			"cat_dev::net::tcp_client::connect(): started connection (TcpStream::connect())",
		);

		let cloned_stream_begin = self.on_stream_begin.clone();
		let cloned_stream_end = self.on_stream_end.clone();
		let cloned_nagle_guard = self.nagle_guard.clone();
		let cloned_slowerloris_timeout = self.slowloris_timeout;
		let streams_ref = self.streams.clone();
		let primary_stream_id_ref = self.primary_stream_id.clone();
		let cloned_chunk_output_at_size = self.chunk_output_at_size;
		let cloned_pre_nagle_hook = self.pre_nagle_hook;
		let cloned_post_nagle_hook = self.post_nagle_hook;
		#[cfg(debug_assertions)]
		let cloned_trace = self.trace_during_debug;
		let cloned_service_name = self.service_name;
		let cloned_cat_dev_slowdown = self.cat_dev_slowdown;

		TaskBuilder::new()
			.name("cat_dev::net::tcp_client::connect().connection.handle")
			.spawn(async move {
				if let Err(cause) = Self::handle_tcp_stream(
					raw_stream,
					stream_id,
					remote_address,
					cloned_stream_begin,
					cloned_stream_end,
					cloned_nagle_guard,
					cloned_slowerloris_timeout,
					streams_ref,
					primary_stream_id_ref,
					cloned_chunk_output_at_size,
					cloned_pre_nagle_hook,
					cloned_post_nagle_hook,
					cloned_cat_dev_slowdown,
					#[cfg(debug_assertions)]
					cloned_trace,
				)
				.instrument(error_span!(
					"CatDevTCPClientConnect",
					client.address = %local_address,
					server.address = %remote_address,
					client.service = cloned_service_name,
					stream.id = stream_id,
					stream.stream_type = "client",
				))
				.await
				{
					warn!(
						?cause,
						client.address = %local_address,
						server.address = %remote_address,
						client.service = cloned_service_name,
						"Error escaped while handling TCP Connection.",
					);
				}
			})
			.map_err(CatBridgeError::SpawnFailure)?;

		Ok(stream_id)
	}

	/// Send a series of bytes over the wire potentially receiving responses back.
	///
	/// This will always only take the response from the 'primary server'. Even if
	/// other servers respond first or at all. It will always be the primary
	/// response.
	///
	/// If you want to truly access all the client streams at once use
	/// [`Self::broadcast_send`] to send to all, and receive all their responses
	/// back.
	///
	/// This _will_ return the stream that was used as the 'primary', the
	/// request-id to wait for a response later or otherwise, and the optional
	/// response if we waited for one.
	///
	/// ## Errors
	///
	/// This function will error if we run into any issues writing or reading the
	/// bytes from the stream in a timely manner.
	pub async fn send<ErrorTy: Debug, BodyTy: TryInto<Bytes, Error = ErrorTy>>(
		&self,
		body: BodyTy,
		wait_for_response_timeout: Option<Duration>,
	) -> Result<(u64, RequestID, Option<Response>), NetworkError> {
		let active_sid = self.get_active_sid().await?;

		// This will be cloned, and modified for each stream we send out too.
		let mut request = Request::new_with_state(
			body.try_into().map_err(|cause| {
				CommonNetClientNetworkError::SerializationError(miette!("{cause:?}"))
			})?,
			SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)),
			(),
			None,
		);
		let req_id = RequestID::generate();
		request.extensions_mut().insert(req_id.clone());

		let mut ids = FnvHashSet::default();
		self.streams
			.scan_async(|stream_id, _stream| {
				ids.insert(*stream_id);
			})
			.await;

		let mut tasks = Vec::with_capacity(ids.len());
		for id in &ids {
			tasks.push(self.send_to_stream(
				*id,
				request.clone(),
				wait_for_response_timeout.unwrap_or(DEFAULT_SLOWLORIS_TIMEOUT),
			));
		}
		// join all gives us a Vec<Result>, we can collect that into one final
		// result, to use.
		join_all(tasks)
			.await
			.into_iter()
			.collect::<Result<(), NetworkError>>()?;

		match wait_for_response_timeout {
			// Don't drain/wait for responses when there are none.
			None | Some(EMPTY_TIMEOUT) => Ok((active_sid, req_id, None)),
			Some(duration) => {
				let mut tasks;
				// If we keep all responsese
				if self.keep_all_responses {
					tasks = vec![self.get_response_from_stream(active_sid, req_id.clone())];
				} else {
					tasks = Vec::with_capacity(ids.len());
					for id in ids {
						tasks.push(self.get_response_from_stream(id, req_id.clone()));
					}
				}
				let responses = timeout(duration, join_all(tasks))
					.await
					.map_err(|_| NetworkError::Timeout(duration))?;

				for (got_stream_id, response) in responses {
					if got_stream_id == active_sid {
						return Ok((active_sid, req_id, response));
					}
				}

				Ok((active_sid, req_id, None))
			}
		}
	}

	/// The equivalent of [`send`], but get all the responses back out of this
	/// client.
	///
	/// ## Errors
	///
	/// If we timeout, or run into any sort of error sending or receiving content
	/// from a stream.
	pub async fn broadcast_send<ErrorTy: Debug, BodyTy: TryInto<Bytes, Error = ErrorTy>>(
		&self,
		body: BodyTy,
		wait_for_response_timeout: Duration,
	) -> Result<FnvHashMap<u64, Option<Response>>, CatBridgeError> {
		// This will be cloned, and modified for each stream we send out too.
		let mut request = Request::new_with_state(
			body.try_into().map_err(|cause| {
				CommonNetClientNetworkError::SerializationError(miette!("{cause:?}"))
			})?,
			SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)),
			(),
			None,
		);
		let req_id = RequestID::generate();
		request.extensions_mut().insert(req_id.clone());

		let mut ids = FnvHashSet::default();
		self.streams
			.scan_async(|stream_id, _stream| {
				ids.insert(*stream_id);
			})
			.await;

		let mut tasks = Vec::with_capacity(ids.len());
		for id in &ids {
			tasks.push(self.send_to_stream(*id, request.clone(), wait_for_response_timeout));
		}
		// join all gives us a Vec<Result>, we can collect that into one final
		// result, to use.
		join_all(tasks)
			.await
			.into_iter()
			.collect::<Result<(), NetworkError>>()?;

		let mut response_tasks = Vec::with_capacity(ids.len());
		for id in &ids {
			response_tasks.push(self.get_response_from_stream(*id, req_id.clone()));
		}
		let responses = timeout(wait_for_response_timeout, join_all(response_tasks))
			.await
			.map_err(|_| NetworkError::Timeout(wait_for_response_timeout))?;

		let mut map =
			FnvHashMap::with_capacity_and_hasher(ids.len(), BuildHasherDefault::default());
		for (got_stream_id, response) in responses {
			map.insert(got_stream_id, response);
		}
		Ok(map)
	}

	/// Receive a packet from the primary stream.
	///
	/// Used for out-of-band receiving of packets that don't have an associated
	/// [`Self::send`] call.
	///
	/// ## Errors
	///
	/// If we timeout, or have another series of failures reading bytes from our
	/// stream.
	pub async fn receive(&self, wait_until: Duration) -> Result<Option<Response>, NetworkError> {
		let active_sid = self.get_active_sid().await?;

		let mut tasks;
		if self.keep_all_responses {
			tasks = vec![self.get_any_response_from_stream(active_sid)];
		} else {
			let mut ids = FnvHashSet::default();
			self.streams
				.scan_async(|stream_id, _stream| {
					ids.insert(*stream_id);
				})
				.await;

			tasks = Vec::with_capacity(ids.len());
			for id in ids {
				tasks.push(self.get_any_response_from_stream(id));
			}
		}
		let responses = timeout(wait_until, join_all(tasks))
			.await
			.map_err(|_| NetworkError::Timeout(wait_until))?;

		for (got_stream_id, _, response) in responses {
			if got_stream_id == active_sid {
				return Ok(response);
			}
		}

		Ok(None)
	}

	/// Get all the responses for a particular request id.
	///
	/// In order to keep memory from ballooning up, and the fact that TCP is
	/// ordered. This API also expects to be called in an 'ordered' way. Previous
	/// requests that do not match the request id will be dropped.
	pub async fn take_all_response_for_request_id(
		&self,
		request_id: &RequestID,
		wait_for: Duration,
	) -> FnvHashMap<u64, Option<Response>> {
		let mut ids = FnvHashSet::default();
		self.streams
			.scan_async(|stream_id, _stream| {
				ids.insert(*stream_id);
			})
			.await;

		let mut tasks = Vec::with_capacity(ids.len());
		for id in &ids {
			tasks.push(timeout(
				wait_for,
				self.get_response_from_stream(*id, request_id.clone()),
			));
		}

		let mut results: FnvHashMap<u64, Option<Response>> =
			join_all(tasks).await.into_iter().flatten().collect();
		for id in ids {
			results.entry(id).or_insert(None);
		}
		results
	}

	#[allow(
		// all of our parameters are very well named, and types are not close to
		// overlapping with each other.
		//
		// we also just fundamenetally have a lot of state thanks to the complexity
		// of all the things we have to handle for a TCP connection, e.g. NAGLE,
		// delimiters, caches, etc.
		//
		// it is also only ever called from one internal function, so it's not like
		// part of our public facing api.
		clippy::too_many_arguments,
	)]
	async fn handle_tcp_stream(
		mut stream: TcpStream,
		stream_id: u64,
		remote_address: SocketAddr,
		on_stream_begin: Option<UnderlyingOnStreamBeginService<()>>,
		on_stream_end: Option<UnderlyingOnStreamEndService<()>>,
		nagle_guard: NagleGuard,
		slowloris_timeout: Duration,
		stream_lists: Arc<ConcurrentHashMap<u64, TCPClientStream>>,
		active_stream_ptr: Arc<AtomicU64>,
		chunk_output_on_size: Option<usize>,
		pre_hook: Option<&'static dyn PreNagleFnTy>,
		post_hook: Option<&'static dyn PostNagleFnTy>,
		cat_dev_slowdown: Option<Duration>,
		#[cfg(debug_assertions)] trace_io: bool,
	) -> Result<(), CatBridgeError> {
		// We drop the 'sender' to cancel the stream. This means we can't hold a
		// copy of any 'Sender' for a long life. So we use this small little block
		// to do that for us.
		let mut receive_packets_to_send: BoundedReceiver<RequestStreamMessage>;
		let (response_sink_send, response_sink_recv) = bounded_channel(128);
		{
			let (mut sender, receiver) = bounded_channel(128);

			// First perform any initialization necessary....
			//
			// And make sure they tell us to continue before doing stuff.
			if Self::initialize_stream(
				on_stream_begin,
				&mut sender,
				&remote_address,
				&stream,
				stream_id,
			)
			.await?
			{
				return Ok(());
			}

			let mut active_stream =
				TCPClientStream::new(remote_address, sender, receiver, response_sink_recv);
			receive_packets_to_send = active_stream
				.steal_send_requests_receiver()
				.ok_or_else(|| CatBridgeError::ClosedChannel)?;

			stream_lists
				.insert_async(stream_id, active_stream)
				.await
				.map_err(|(stream_id, _stream)| NetworkError::DuplicateStreamId(stream_id))?;
			// Update the active stream pointer if need be.
			_ = active_stream_ptr.compare_exchange(
				0,
				stream_id,
				Ordering::AcqRel,
				Ordering::Acquire,
			);
		}

		// Connection has been "approved", setup the on disconnect handler.
		let _guard = on_stream_end
			.map(|service| DisconnectAsyncDropClient::new(service, (), remote_address, stream_id));

		let mut buff = BytesMut::with_capacity(TCP_READ_BUFFER_SIZE);
		// Any previously saved data that was a victim of NAGLE's algorithim, or
		// similar.
		let mut nagle_cache: Option<(BytesMut, SystemTime)> = None;
		let mut cached_request_id: Option<RequestID> = None;

		loop {
			tokio::select! {
				opt = receive_packets_to_send.recv() => {
					// Sender is closed, shutdown our channel cleanly.
					if Self::handle_client_write_to_connection(
						chunk_output_on_size,
						opt,
						pre_hook,
						&mut cached_request_id,
						stream_id,
						&mut stream,
						cat_dev_slowdown,
						#[cfg(debug_assertions)]
						trace_io,
					).await? {
						break;
					}
				}
				read_res = stream.read_buf(&mut buff) => {
					let read_bytes = read_res.map_err(NetworkError::IO)?;
					buff.truncate(read_bytes);

					if buff.is_empty() {
						buff = BytesMut::with_capacity(TCP_READ_BUFFER_SIZE);
						continue;
					}

					if Self::handle_client_read_from_connection(
						buff,
						&nagle_guard,
						slowloris_timeout,
						&mut nagle_cache,
						response_sink_send.clone(),
						post_hook,
						&mut cached_request_id,
						stream_id,
						#[cfg(debug_assertions)]
						trace_io,
					).await? {
						break;
					}
					buff = BytesMut::with_capacity(TCP_READ_BUFFER_SIZE);
				}
			}
		}

		Ok(())
	}

	async fn initialize_stream(
		on_stream_begin_handler: Option<UnderlyingOnStreamBeginService<()>>,
		send_channel: &mut BoundedSender<RequestStreamMessage>,
		remote_address: &SocketAddr,
		tcp_stream: &TcpStream,
		stream_id: u64,
	) -> Result<bool, CatBridgeError> {
		tcp_stream.set_nodelay(true).map_err(NetworkError::IO)?;

		if let Some(mut handle) = on_stream_begin_handler {
			if !handle
				.call(RequestStreamEvent::new_with_state(
					send_channel.clone(),
					*remote_address,
					Some(stream_id),
					(),
				))
				.await?
			{
				trace!("handler failed on stream begin hook");
				return Ok(true);
			}
		}

		Ok(false)
	}

	#[allow(
		// All of our types are very differently typed, and well named, so chance
		// of confusion is low.
		//
		// Not to mention this is an internal only method.
		clippy::too_many_arguments,
	)]
	async fn handle_client_read_from_connection<'data>(
		mut buff: BytesMut,
		nagle_guard: &'data NagleGuard,
		slowloris_timeout: Duration,
		nagle_cache: &'data mut Option<(BytesMut, SystemTime)>,
		response_output: BoundedSender<(Option<RequestID>, Response)>,
		cloned_post_nagle: Option<&'static dyn PostNagleFnTy>,
		cached_request_id: &mut Option<RequestID>,
		stream_id: u64,
		#[cfg(debug_assertions)] trace_io: bool,
	) -> Result<bool, CatBridgeError> {
		if let Some(convert_fn) = cloned_post_nagle {
			buff = BytesMut::from(block_in_place(|| (*convert_fn)(stream_id, buff.freeze())));
		}

		#[cfg(debug_assertions)]
		{
			if trace_io {
				debug!(
					body.hex = format!("{:02x?}", buff),
					body.str = String::from_utf8_lossy(&buff).to_string(),
					"cat-dev-trace-input-tcp-client",
				);
			}
		}

		// We may be NAGEL'd, so we need to recover/split, and potentially buffer
		// any packets. Also watch out for slowloris-esque attacks.
		let start_time = now();
		if let Some((mut existing_buff, old_start_time)) = nagle_cache.take() {
			// If we can't calculat duration seconds it's negative, or no duration
			// has passed yet.
			//
			// Just treat it as 0.
			let total_duration = start_time
				.duration_since(old_start_time)
				.unwrap_or(Duration::from_secs(0));
			if total_duration > slowloris_timeout {
				debug!(
					cause = ?CommonNetNetworkError::SlowlorisTimeout(total_duration),
					"slowloris-detected",
				);
				return Ok(true);
			}

			existing_buff.extend(buff);
			buff = existing_buff;
		}

		while let Some((start_of_packet, end_of_packet)) = nagle_guard.split(&buff)? {
			let remaining_buff = buff.split_off(end_of_packet);
			let _start_of_buff = buff.split_to(start_of_packet);
			let req_body = buff.freeze();
			buff = remaining_buff;

			if let Err(cause) = response_output
				.send((cached_request_id.take(), Response::new_with_body(req_body)))
				.await
			{
				warn!(
					?cause,
					"internal queue failure will not send disconnect/response."
				);
			}
		}

		if !buff.is_empty() {
			_ = nagle_cache.insert((buff, start_time));
		}

		Ok(false)
	}

	#[allow(
		// Well typed arguments, lots to do and all that.
		clippy::too_many_arguments,
	)]
	async fn handle_client_write_to_connection(
		chunk_output_on_size: Option<usize>,
		to_send_to_client_opt: Option<RequestStreamMessage>,
		pre_hook: Option<&'static dyn PreNagleFnTy>,
		cached_request_id: &mut Option<RequestID>,
		stream_id: u64,
		raw_stream: &mut TcpStream,
		cat_dev_slowdown: Option<Duration>,
		#[cfg(debug_assertions)] trace_io: bool,
	) -> Result<bool, CatBridgeError> {
		let Some(to_send_to_client) = to_send_to_client_opt else {
			return Ok(true);
		};

		match to_send_to_client {
			RequestStreamMessage::Disconnect => {
				// Clear a value, it's a disconnect.
				_ = cached_request_id.take();
				trace!("stream-disconnect-message");
				Ok(true)
			}
			RequestStreamMessage::Request(mut req) => {
				if !req.body().is_empty() {
					if let Ok(req_id) = RequestID::from_request_parts(&mut req).await {
						_ = cached_request_id.insert(req_id);
					}
					let messages = if let Some(size) = chunk_output_on_size {
						req.body_owned()
							.chunks(size)
							.map(BytesMut::from)
							.collect::<Vec<_>>()
					} else {
						vec![BytesMut::from(req.body_owned())]
					};

					for message in messages {
						#[cfg(debug_assertions)]
						if trace_io {
							debug!(
								body.hex = format!("{message:02x?}"),
								body.str = String::from_utf8_lossy(&message).to_string(),
								"cat-dev-trace-output-tcp-client",
							);
						}

						let mut full_response = message.clone();
						if let Some(pre) = pre_hook {
							block_in_place(|| pre(stream_id, &mut full_response));
						}
						if let Some(slowdown) = cat_dev_slowdown {
							sleep(slowdown).await;
						}

						raw_stream.writable().await.map_err(NetworkError::IO)?;
						raw_stream
							.write_all(&full_response)
							.await
							.map_err(NetworkError::IO)?;
					}
				}

				Ok(false)
			}
		}
	}

	/// Send a request to a stream if it exists. It may not exist.
	///
	/// ## Errors
	///
	/// If we fail to queue up a packet to be sent out over a stream.
	async fn send_to_stream(
		&self,
		stream_id: u64,
		mut base_request: Request<()>,
		timeout: Duration,
	) -> Result<(), NetworkError> {
		if let Some(stream) = self.streams.get_async(&stream_id).await {
			base_request.update_request_source(stream.server_address(), Some(stream_id));
			stream
				.send_timeout(RequestStreamMessage::Request(base_request), timeout)
				.await
				.map_err(|cause| CommonNetClientNetworkError::CannotQueueSend(cause).into())
		} else {
			// Stream must've gotten removed since we got our list.
			Ok(())
		}
	}

	/// Receive a response from a particular stream if it exists, it may not
	/// exist.
	async fn get_any_response_from_stream(
		&self,
		stream_id: u64,
	) -> (u64, Option<RequestID>, Option<Response>) {
		if let Some(mut stream) = self.streams.get_async(&stream_id).await {
			let Some((opt_req_id, response)) = stream.response_channel_mut().recv().await else {
				return (stream_id, None, None);
			};

			(stream_id, opt_req_id, Some(response))
		} else {
			// Stream must've gotten removed since we got our list.
			(stream_id, None, None)
		}
	}

	/// Receive a response from a particular stream if it exists, it may not
	/// exist.
	async fn get_response_from_stream(
		&self,
		stream_id: u64,
		request_id: RequestID,
	) -> (u64, Option<Response>) {
		if let Some(mut stream) = self.streams.get_async(&stream_id).await {
			while let Some((opt_req_id, response)) = stream.response_channel_mut().recv().await {
				if let Some(got_req_id) = opt_req_id {
					if got_req_id == request_id {
						return (stream_id, Some(response));
					}
				}
			}

			(stream_id, None)
		} else {
			// Stream must've gotten removed since we got our list.
			(stream_id, None)
		}
	}

	/// Get the active stream id to use for connections.
	async fn get_active_sid(&self) -> Result<u64, CommonNetClientNetworkError> {
		let active_sid = self.primary_stream_id.load(Ordering::Acquire);
		if active_sid == 0 {
			return Err(CommonNetClientNetworkError::NotConnectedToServer);
		}

		if !self.streams.contains(&active_sid) {
			let mut oldest_stream = None;

			self.streams
				.scan_async(|stream_id, stream| {
					if let Some((_strm_id, strm_created_at)) = oldest_stream {
						if stream.opened_at() < strm_created_at {
							_ = oldest_stream.insert((*stream_id, stream.opened_at()));
						}
					} else {
						_ = oldest_stream.insert((*stream_id, stream.opened_at()));
					}
				})
				.await;
		}

		Ok(active_sid)
	}
}

impl Debug for TCPClient {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		let mut tcp_dbg_struct = fmt.debug_struct("TCPClient");

		tcp_dbg_struct
			.field("cat_dev_slowdown", &self.cat_dev_slowdown)
			.field("chunk_output_at_size", &self.chunk_output_at_size)
			.field("keep_all_responses", &self.keep_all_responses)
			.field("nagle_guard", &self.nagle_guard)
			.field("has_on_stream_begin", &self.on_stream_begin.is_some())
			.field("has_on_stream_end", &self.on_stream_end.is_some())
			.field("has_pre_nagle_hook", &self.pre_nagle_hook.is_some())
			.field("has_post_nagle_hook", &self.post_nagle_hook.is_some())
			.field(
				"primary_stream_id",
				&self.primary_stream_id.load(Ordering::Relaxed),
			)
			.field("streams", &self.streams)
			.field("service_name", &self.service_name)
			.field("slowloris_timeout", &self.slowloris_timeout);

		#[cfg(debug_assertions)]
		{
			tcp_dbg_struct.field("trace_during_debug", &self.trace_during_debug);
		}

		tcp_dbg_struct.finish()
	}
}

const TCP_CLIENT_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("cat_dev_slowdown"),
	NamedField::new("chunk_output_at_size"),
	NamedField::new("keep_all_responses"),
	NamedField::new("nagle_guard"),
	NamedField::new("has_on_stream_begin"),
	NamedField::new("has_on_stream_end"),
	NamedField::new("has_pre_nagle_hook"),
	NamedField::new("has_post_nagle_hook"),
	NamedField::new("primary_stream_id"),
	NamedField::new("streams"),
	NamedField::new("service_name"),
	NamedField::new("slowloris_timeout"),
	#[cfg(debug_assertions)]
	NamedField::new("trace_during_debug"),
];

impl Structable for TCPClient {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static("TCPClient", Fields::Named(TCP_CLIENT_FIELDS))
	}
}

impl Valuable for TCPClient {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		let mut valuable_map = FnvHashMap::default();
		self.streams.scan(|stream_id, stream| {
			valuable_map.insert(*stream_id, stream.to_valuable());
		});

		visitor.visit_named_fields(&NamedValues::new(
			TCP_CLIENT_FIELDS,
			&[
				Valuable::as_value(&if let Some(slowdown) = self.cat_dev_slowdown {
					format!("{}ms", slowdown.as_millis())
				} else {
					"<none>".to_string()
				}),
				Valuable::as_value(&self.chunk_output_at_size),
				Valuable::as_value(&self.keep_all_responses),
				Valuable::as_value(&self.nagle_guard),
				Valuable::as_value(&self.on_stream_begin.is_some()),
				Valuable::as_value(&self.on_stream_end.is_some()),
				Valuable::as_value(&self.pre_nagle_hook.is_some()),
				Valuable::as_value(&self.post_nagle_hook.is_some()),
				Valuable::as_value(&self.primary_stream_id.load(Ordering::Relaxed)),
				Valuable::as_value(&valuable_map),
				Valuable::as_value(&self.service_name),
				Valuable::as_value(&self.slowloris_timeout.as_secs()),
				#[cfg(debug_assertions)]
				Valuable::as_value(&self.trace_during_debug),
			],
		));
	}
}

/// An active TCP Client stream.
///
/// This represents a stream that is actively processing. When it drops (and by
/// proxy drops it's Sender) the task will notice it's receiver has closed, and
/// will automatically close tiself.
struct TCPClientStream {
	/// The remote address of this stream.
	remote_address: SocketAddr,
	/// The actual responses coming from this TCP Stream.
	response_channel: BoundedReceiver<(Option<RequestID>, Response)>,
	/// The receiver for this particular stream. Gets stolen by the worker
	/// actually handling tasks.
	send_requests_receiver: Option<BoundedReceiver<RequestStreamMessage>>,
	/// The sender to send requests to this stream.
	send_requests: BoundedSender<RequestStreamMessage>,
	/// The instant this stream was opened, used for sorting streams.
	time_opened: Instant,
}

impl TCPClientStream {
	/// Create a new TCP Client stream given a sender/receiver pair.
	#[must_use]
	pub fn new(
		remote_address: SocketAddr,
		sender: BoundedSender<RequestStreamMessage>,
		receiver: BoundedReceiver<RequestStreamMessage>,
		response_channel: BoundedReceiver<(Option<RequestID>, Response)>,
	) -> Self {
		Self {
			remote_address,
			response_channel,
			send_requests_receiver: Some(receiver),
			send_requests: sender,
			time_opened: Instant::now(),
		}
	}

	#[must_use]
	pub const fn to_valuable(&self) -> TCPClientStreamValuable {
		TCPClientStreamValuable {
			receiver_stolen: self.send_requests_receiver.is_none(),
			time_opened: self.time_opened,
		}
	}

	/// The address of the server on the other side.
	pub const fn server_address(&self) -> SocketAddr {
		self.remote_address
	}

	#[must_use]
	pub const fn response_channel_mut(
		&mut self,
	) -> &mut BoundedReceiver<(Option<RequestID>, Response)> {
		&mut self.response_channel
	}

	/// Steal the receiver if it already hasn't been stolen already.
	#[must_use]
	pub fn steal_send_requests_receiver(
		&mut self,
	) -> Option<BoundedReceiver<RequestStreamMessage>> {
		self.send_requests_receiver.take()
	}

	/// Send a message to the client stream with a given timeout.
	pub async fn send_timeout(
		&self,
		message: RequestStreamMessage,
		timeout: Duration,
	) -> Result<(), SendTimeoutError<RequestStreamMessage>> {
		self.send_requests.send_timeout(message, timeout).await
	}

	/// When this stream was opened.
	pub const fn opened_at(&self) -> Instant {
		self.time_opened
	}
}

impl Debug for TCPClientStream {
	fn fmt(&self, fmt: &mut Formatter) -> FmtResult {
		fmt.debug_struct("TCPClientStream")
			.field("receiver_stolen", &self.send_requests_receiver.is_none())
			.field("time_opened", &self.time_opened)
			.finish_non_exhaustive()
	}
}

impl PartialEq for TCPClientStream {
	fn eq(&self, other: &Self) -> bool {
		self.time_opened == other.time_opened
	}
}

impl PartialOrd for TCPClientStream {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		Some(self.time_opened.cmp(&other.time_opened))
	}
}

const TCP_CLIENT_STREAM_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("receiver_stolen"),
	NamedField::new("time_opened"),
];

impl Structable for TCPClientStream {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static("TCPClientStream", Fields::Named(TCP_CLIENT_STREAM_FIELDS))
	}
}

impl Valuable for TCPClientStream {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			TCP_CLIENT_STREAM_FIELDS,
			&[
				Valuable::as_value(&self.send_requests_receiver.is_none()),
				Valuable::as_value(
					&SystemTime::now()
						.checked_add(self.time_opened.elapsed())
						.unwrap_or_else(SystemTime::now)
						.duration_since(SystemTime::UNIX_EPOCH)
						.unwrap_or_default()
						.as_secs(),
				),
			],
		));
	}
}

struct TCPClientStreamValuable {
	receiver_stolen: bool,
	time_opened: Instant,
}

impl Structable for TCPClientStreamValuable {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static("TCPClientStream", Fields::Named(TCP_CLIENT_STREAM_FIELDS))
	}
}

impl Valuable for TCPClientStreamValuable {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			TCP_CLIENT_STREAM_FIELDS,
			&[
				Valuable::as_value(&self.receiver_stolen),
				Valuable::as_value(
					&SystemTime::now()
						.checked_add(self.time_opened.elapsed())
						.unwrap_or_else(SystemTime::now)
						.duration_since(SystemTime::UNIX_EPOCH)
						.unwrap_or_default()
						.as_secs(),
				),
			],
		));
	}
}
