//! Utilities for Serving the TCP layer 4 protocol.
//!
//! Note this "TCP Server" class allows both binding to a particular port, as
//! well as "connect"'ing to another host. This is mostly due to the fact we
//! have to support SDIO where the person who "connect"'s acts as the server.
//!
//! ## Notes About Layering
//!
//! When layer-ing things on-top of service, it's important to note about
//! performance characteristics. For every single individual `layer` call
//! there ***may be significant overhead***. As such, it is best to use
//! [`tower::ServiceBuilder`] in order to lower the cost of overhead related
//! to significant layering.
//!
//! ## TCP and NAGLE'ing
//!
//! TCP the protocol is fundamentally built on-top of a 'stream', and is
//! not packet oriented. Even though we try to guard against this, by
//! setting no-delay on multiple parts of our infrastructure. There is
//! a whole world out there who may not do what we want.
//!
//! As a result, we need to guard against weird splits of data across read
//! calls. Because a single read call isn't neccissarily going to read a full
//! "packet", and may even read multiple packets.
//!
//! I generally will refer to this in the source code as "NAGLE" protections,
//! as NAGLE's algorithim is usually to blame for weird behaviors here. Where
//! a device will combine multiple small packets together.
//!
//! ## SLOW-loris
//!
//! As mentioned TCP is built ontop of a stream, there is a chance that someone
//! can just "slow drip" us a packet, bit by bit, eating up an open connection
//! slot for a long period of time. Eating up a connection slot forever. This
//! used to be a lot more damaging when you'd allocate a thread for every
//! single connection.
//!
//! For us, because we _don't_ allocate a thread for every single connection
//! the cost of any single connection is low. Not to mention we don't really
//! have to deal with untrusted DOS connections in a lot of cases.
//!
//! That being said, there's no reason to allow someone to slow-drip us a
//! packet forever, so our servers will inherently time out a packet if it's
//! taking too long closing a connection.
//!
//! ## Lifecycle of a Packet
//!
//! ```ascii
//!     ┌─────────┐                        ┌─────────┐
//!     │TCPClient│                        │TCPServer│
//!     └─────────┘                        └─────────┘
//!          │Sends Arbitrary Sequence of Bytes │
//!          │─────────────────────────────────>│
//!          │                                  │
//!          │                                  │────┐
//!          │                                  │    │ Call PreNagleFN (if one exists).
//!          │                                  │    │ This is a custom function that handles things like:
//!          │                                  │    │
//!          │                                  │    │   1. Decrypting Packets.
//!          │                                  │    │   2. Logging info not through trace.
//!          │                                  │    │
//!          │                                  │    │ Generally should not be used for anything as it
//!          │                                  │    │ happens before ALL processing.
//!          │                                  │<───┘
//!          │                                  │
//!          │                                  │────┐
//!          │                                  │    │ Check how long this client has been attempting to
//!          │                                  │    │ send us a single packet.
//!          │                                  │    │
//!          │                                  │    │ Compare this with `SLOWLORIS_TIMEOUT`,
//!          │                                  │    │ and error/close the stream if it's taken too long to
//!          │                                  │    │ send us a packet).
//!          │                                  │<───┘
//!          │                                  │
//!          │                                  │────┐
//!          │                                  │    │ Look at "nagle cache", this is any data that was previously sent
//!          │                                  │    │ on this TCP Stream, that has not yet been processed.
//!          │                                  │    │
//!          │                                  │    │ Create final `buff`.
//!          │                                  │<───┘
//!          │                                  │
//!          │                                  │────┐
//!          │                                  │    │ Look at "nagle guard", this determines when one packet begins/ends.
//!          │                                  │    │ Process any # of packets that may have come in.
//!          │                                  │    │
//!          │                                  │    │ If any data is left over put it in "nagle cache".
//!          │                                  │<───┘
//!          │                                  │
//!          │                                  │────┐
//!          │                                  │    │ Process Packet.
//!          │                                  │<───┘
//!          │                                  │
//!          │                                  │────┐
//!          │                                  │    │ If reply was sent, and wasn't empty...
//!          │                                  │<───┘
//!          │                                  │
//!          │                                  │────┐
//!          │                                  │    │ Look at "chunked output size", and chunk packet if necessary.
//!          │                                  │<───┘
//!          │                                  │
//!          │                                  │────┐
//!          │                                  │    │ For each packet apply PostNagleFN (if one exists).
//!          │                                  │    │ This is a custom function that handles things like:
//!          │                                  │    │
//!          │                                  │    │   1. Encrypting Packets.
//!          │                                  │    │   2. Logging info not through trace.
//!          │                                  │    │   3. Sleeping to not overwhelm a CAT-DEV.
//!          │                                  │<───┘
//!          │                                  │
//!          │      Data for the Client.        │
//!          │<─────────────────────────────────│
//!     ┌─────────┐                        ┌─────────┐
//!     │TCPClient│                        │TCPServer│
//!     └─────────┘                        └─────────┘
//! ```

use crate::{
	errors::{CatBridgeError, NetworkError},
	net::{
		DEFAULT_SLOWLORIS_TIMEOUT, STREAM_ID, TCP_READ_BUFFER_SIZE,
		errors::{CommonNetAPIError, CommonNetNetworkError},
		handlers::{
			OnResponseStreamBeginHandler, OnResponseStreamEndHandler,
			OnStreamBeginHandlerAsService, OnStreamEndHandlerAsService,
		},
		models::{NagleGuard, PostNagleFnTy, PreNagleFnTy, Request, Response},
		now,
		server::models::{
			DisconnectAsyncDropServer, ResponseStreamEvent, ResponseStreamMessage,
			UnderlyingOnStreamBeginService, UnderlyingOnStreamEndService,
		},
	},
};
use bytes::{Bytes, BytesMut};
use fnv::FnvHashSet;
use futures::future::join_all;
use scc::HashMap as ConcurrentMap;
use std::{
	convert::Infallible,
	fmt::{Debug, Formatter, Result as FmtResult},
	net::SocketAddr,
	sync::{Arc, LazyLock, atomic::Ordering},
	time::{Duration, SystemTime},
};
use tokio::{
	io::{AsyncReadExt, AsyncWriteExt},
	net::{TcpListener, TcpStream, ToSocketAddrs, lookup_host},
	sync::{
		Mutex,
		mpsc::{Sender as BoundedSender, channel as bounded_channel},
	},
	task::{Builder as TaskBuilder, block_in_place},
	time::sleep,
};
use tower::{Layer, Service, util::BoxCloneService};
use tracing::{Instrument, debug, error_span, trace, warn};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

#[cfg(debug_assertions)]
use crate::net::SPRIG_TRACE_IO;

/// A map of streams and the channels to queue a repsonse packet out on, in a
/// complete out of band way.
static OUT_OF_BAND_SENDERS: LazyLock<ConcurrentMap<u64, BoundedSender<ResponseStreamMessage>>> =
	LazyLock::new(ConcurrentMap::new);

/// An implementation of a TCP server.
///
/// This is the "base" TCP Server class that any of the servers that cat-dev
/// will be built on. It has extensions for handling any potential type of
/// server that we will ever need to support.
///
/// This server accepts lots of parameters so look at the modules documentation
/// for general info about how processing works.
pub struct TCPServer<State: Clone + Send + Sync + 'static = ()> {
	/// The address that the server will bound too, when [`bind`]
	/// is called. It may also be a server address to connect too when
	/// we are a server who connects to our client.
	///
	/// Currently our server can only bind to a very specific address, and does
	/// not supporting binding to multiple interfaces besides the special
	/// `0.0.0.0` addresses that get handled by the OS.
	address_to_bind_or_connect_to: SocketAddr,
	/// Cat-dev's need load-bearing sleeps as they can "ACK" a ppacket, but
	/// throw away the bytes and pretend it never got implied.
	///
	/// This is used for sleeping before then. By default this isn't set,
	/// cat-dev services will call: [`TCPServer::set_cat_dev_slowdown`].
	cat_dev_slowdown: Option<Duration>,
	/// For devices that can't receive too much data at once.
	///
	/// When devices *cough* MION *cough* can't receive too much data at once,
	/// we need to chunk it, and rest between those chunks. This will chunk those
	/// packets for us.
	chunk_output_at_size: Option<usize>,
	/// The [`Service`] that gets called when a packet is received.
	///
	/// This will almost always be some sort of router that knows how to route
	/// packets to handlers. However, it could just be a single tower service.
	initial_service: BoxCloneService<Request<State>, Response, Infallible>,
	/// Determines when a packet starts, and ends.
	nagle_guard: NagleGuard,
	/// A tower service to call when a particular stream starts.
	///
	/// This is effectively like an "on connect" hook that you can use to
	/// call functions.
	on_stream_begin: Option<UnderlyingOnStreamBeginService<State>>,
	/// A tower service to call when a particular stream ends.
	///
	/// This is effectively like an "on disconnect" hook that you can use
	/// to call functions.
	on_stream_end: Option<UnderlyingOnStreamEndService<State>>,
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
	/// The name of the service being provided by this server, to attach to logs.
	service_name: &'static str,
	/// The "Slowloris Detection" timeout.
	slowloris_timeout: Duration,
	/// Type-safe server wide state.
	///
	/// This is mostly used by our servers to attach state this is meant to be
	/// server wide. Such as a `HostFilesystem` type to abstract out all the
	/// filesystem stuff that many of our servers have to do.
	state: State,
	/// If we should log all packet requests/responses when compiled with debug
	/// assertions.
	#[cfg(debug_assertions)]
	trace_during_debug: bool,
}

impl TCPServer<()> {
	/// Create a new TCP Server without having to specify any particular state.
	///
	/// ## Errors
	///
	/// If we cannot look up one specific address to bind too.
	pub async fn new<AddrTy, ServiceTy>(
		service_name: &'static str,
		bind_addr: AddrTy,
		initial_service: ServiceTy,
		nagle_hooks: (
			Option<&'static dyn PreNagleFnTy>,
			Option<&'static dyn PostNagleFnTy>,
		),
		guard: impl Into<NagleGuard>,
		trace_io_during_debug: bool,
	) -> Result<Self, CommonNetAPIError>
	where
		AddrTy: ToSocketAddrs,
		ServiceTy:
			Clone + Send + Service<Request<()>, Response = Response, Error = Infallible> + 'static,
		ServiceTy::Future: Send + 'static,
	{
		Self::new_with_state(
			service_name,
			bind_addr,
			initial_service,
			nagle_hooks,
			guard,
			(),
			trace_io_during_debug,
		)
		.await
	}
}

impl<State: Clone + Send + Sync + 'static> TCPServer<State> {
	/// Send a message completely out of band to a particular open stream.
	///
	/// ## Errors
	///
	/// - If the stream is not actively open.
	/// - If we could not actively queue a packet to go out.
	pub async fn out_of_bound_send(
		stream_id: u64,
		message: ResponseStreamMessage,
	) -> Result<(), CatBridgeError> {
		if let Some(stream) = OUT_OF_BAND_SENDERS.get_async(&stream_id).await {
			stream
				.send(message)
				.await
				.map_err(NetworkError::SendQueueMessageFailure)?;
			Ok(())
		} else {
			Err(CommonNetNetworkError::StreamNoLongerProcessing.into())
		}
	}

	/// Send a message completely out of band to all open stream.
	pub async fn out_of_bound_broadcast(
		message: ResponseStreamMessage,
	) -> Vec<Result<(), CatBridgeError>> {
		let mut ids = FnvHashSet::default();
		// Scan all senders at a Point-in-Time..
		OUT_OF_BAND_SENDERS
			.scan_async(|key, _value| {
				ids.insert(*key);
			})
			.await;

		// Now we broadcast to those streams....
		let mut tasks = Vec::with_capacity(ids.len());
		for id in ids {
			tasks.push(Self::out_of_bound_send(id, message.clone()));
		}

		join_all(tasks).await
	}

	/// Create a new TCP Server along with the state for this server.
	///
	/// ## Errors
	///
	/// If we cannot look up one specific address to bind too.
	#[allow(unused)]
	pub async fn new_with_state<AddrTy, ServiceTy>(
		service_name: &'static str,
		bind_addr: AddrTy,
		initial_service: ServiceTy,
		nagle_hooks: (
			Option<&'static dyn PreNagleFnTy>,
			Option<&'static dyn PostNagleFnTy>,
		),
		guard: impl Into<NagleGuard>,
		state: State,
		trace_io_during_debug: bool,
	) -> Result<Self, CommonNetAPIError>
	where
		AddrTy: ToSocketAddrs,
		ServiceTy: Clone
			+ Send
			+ Service<Request<State>, Response = Response, Error = Infallible>
			+ 'static,
		ServiceTy::Future: Send + 'static,
	{
		let hosts = lookup_host(bind_addr)
			.await
			.map_err(CommonNetAPIError::AddressLookupError)?
			.collect::<Vec<_>>();
		if hosts.len() != 1 {
			return Err(CommonNetAPIError::WrongAmountOfAddressesToBindToo(hosts));
		}

		#[cfg(not(debug_assertions))]
		{
			if trace_io_during_debug {
				warn!(
					"Trace IO was turned on, but debug assertsions were not compiled in. Tracing of I/O will not happen. Please recompile cat-dev with debug assertions to properly trace I/O.",
				);
			}
		}

		Ok(Self {
			address_to_bind_or_connect_to: hosts[0],
			cat_dev_slowdown: None,
			chunk_output_at_size: None,
			initial_service: BoxCloneService::new(initial_service),
			nagle_guard: guard.into(),
			on_stream_begin: None,
			on_stream_end: None,
			pre_nagle_hook: nagle_hooks.0,
			post_nagle_hook: nagle_hooks.1,
			service_name,
			slowloris_timeout: DEFAULT_SLOWLORIS_TIMEOUT,
			state,
			#[cfg(debug_assertions)]
			trace_during_debug: trace_io_during_debug || *SPRIG_TRACE_IO,
		})
	}

	/// Get the port that we're either binding too, or connecting too.
	#[must_use]
	pub const fn port(&self) -> u16 {
		self.address_to_bind_or_connect_to.port()
	}

	/// Set the slowdown to before sending bytes from this server.
	pub const fn set_cat_dev_slowdown(&mut self, slowdown: Option<Duration>) {
		self.cat_dev_slowdown = slowdown;
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
	pub const fn on_stream_begin(&self) -> Option<&UnderlyingOnStreamBeginService<State>> {
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
		on_start: Option<UnderlyingOnStreamBeginService<State>>,
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
		HandlerTy: OnResponseStreamBeginHandler<HandlerParamsTy, State> + Clone + Send + 'static,
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
			+ Service<ResponseStreamEvent<State>, Response = bool, Error = CatBridgeError>
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
		LayerTy: Layer<UnderlyingOnStreamBeginService<State>, Service = ServiceTy>,
		ServiceTy: Service<ResponseStreamEvent<State>, Response = bool, Error = CatBridgeError>
			+ Clone
			+ Send
			+ 'static,
		<LayerTy::Service as Service<ResponseStreamEvent<State>>>::Future: Send + 'static,
	{
		let Some(srvc) = self.on_stream_begin.take() else {
			return Err(CommonNetAPIError::OnStreamBeginNotRegistered);
		};

		self.on_stream_begin = Some(BoxCloneService::new(layer.layer(srvc)));
		Ok(())
	}

	#[must_use]
	pub const fn on_stream_end(&self) -> Option<&UnderlyingOnStreamEndService<State>> {
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
		on_end: Option<UnderlyingOnStreamEndService<State>>,
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
		HandlerTy: OnResponseStreamEndHandler<HandlerParamsTy, State> + Clone + Send + 'static,
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
			+ Service<ResponseStreamEvent<State>, Response = (), Error = CatBridgeError>
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
		LayerTy: Layer<UnderlyingOnStreamEndService<State>, Service = ServiceTy>,
		ServiceTy: Service<ResponseStreamEvent<State>, Response = (), Error = CatBridgeError>
			+ Clone
			+ Send
			+ 'static,
		<LayerTy::Service as Service<ResponseStreamEvent<State>>>::Future: Send + 'static,
	{
		let Some(srvc) = self.on_stream_end.take() else {
			return Err(CommonNetAPIError::OnStreamEndNotRegistered);
		};

		self.on_stream_end = Some(BoxCloneService::new(layer.layer(srvc)));
		Ok(())
	}

	#[must_use]
	pub const fn initial_service(&self) -> &BoxCloneService<Request<State>, Response, Infallible> {
		&self.initial_service
	}

	pub fn layer_initial_service<LayerTy, ServiceTy>(&mut self, layer: LayerTy)
	where
		LayerTy: Layer<BoxCloneService<Request<State>, Response, Infallible>, Service = ServiceTy>,
		ServiceTy: Service<Request<State>, Response = Response, Error = Infallible>
			+ Clone
			+ Send
			+ 'static,
		<LayerTy::Service as Service<Request<State>>>::Future: Send + 'static,
	{
		self.initial_service = BoxCloneService::new(layer.layer(self.initial_service.clone()));
	}

	/// Get a reference to the current state of the server.
	#[must_use]
	pub const fn state(&self) -> &State {
		&self.state
	}

	/// "Connect" to our remote client address, and start serving ourselves to
	/// whoever we connect too.
	///
	/// NOTE: this will ***NOT*** return until the service either runs into an
	/// error, or the task is cancelled.
	///
	/// ## Errors
	///
	/// If we cannot connect to the remote source, fetch the local address, or
	/// handle the connection in some other way.
	pub async fn connect(self) -> Result<(), CatBridgeError> {
		loop {
			// Copy the address we're bound to, to use in log statements later on.
			let client_address = self.address_to_bind_or_connect_to;
			let stream = TcpStream::connect(self.address_to_bind_or_connect_to)
				.await
				.map_err(NetworkError::IO)?;
			let loggable_address = stream.local_addr().map_err(NetworkError::IO)?;
			let stream_id = STREAM_ID.fetch_add(1, Ordering::SeqCst);
			trace!(
				server.address = %loggable_address,
				client.address = %client_address,
				stream.id = stream_id,
				stream.stream_type = "server",
				"cat_dev::net::tcp_server::connect(): started connection (TcpStream::connect())",
			);

			if let Err(cause) = Self::handle_tcp_connection(
				self.on_stream_begin.clone(),
				self.on_stream_end.clone(),
				self.nagle_guard.clone(),
				self.slowloris_timeout,
				self.initial_service.clone(),
				stream,
				client_address,
				self.pre_nagle_hook,
				self.post_nagle_hook,
				self.chunk_output_at_size,
				self.state.clone(),
				stream_id,
				self.cat_dev_slowdown,
				#[cfg(debug_assertions)]
				self.trace_during_debug,
			)
			.instrument(error_span!(
				"CatDevTCPServerConnect",
				client.address = %client_address,
				server.address = %loggable_address,
				server.service = self.service_name,
				stream.id = stream_id,
				stream.stream_type = "server",
			))
			.await
			{
				warn!(
					?cause,
					client.address = %client_address,
					server.address = %loggable_address,
					server.service = self.service_name,
					"Error escaped while handling TCP connection.",
				);
			}
		}
	}

	/// Bind this server, and start listening on the specified address.
	///
	/// NOTE: this will ***NOT*** return until the service either runs into an
	/// error, or gets cancelled.
	///
	/// ## Errors
	///
	/// - If we cannot bind to the requested address, and port.
	/// - If we run into an error accept'ing a TCP connection.
	pub async fn bind(self) -> Result<(), CatBridgeError> {
		// Copy the address we're bound to, to use in log statements later on.
		let loggable_address = self.address_to_bind_or_connect_to;
		let listener = TcpListener::bind(self.address_to_bind_or_connect_to)
			.await
			.map_err(NetworkError::IO)?;

		loop {
			let (stream, client_address) = listener.accept().await.map_err(NetworkError::IO)?;
			trace!(
				server.address = %loggable_address,
				client.address = %client_address,
				"cat_dev::net::tcp_server::bind(): received connection (listener.accept())",
			);

			// We need to clone all of our stuff, so they can be owned by the
			// underlying connection task.
			//
			// We spawn a task per connection that way one connection doesn't block
			// the accept'ing of any new connections. If we did, we'd only be able
			// to handle one connection at a time which is lame.
			//
			// Tasks are also incredibly cheap, so spawning one per connection even
			// long lived tasks is nbd.
			let cloned_begin_handler = self.on_stream_begin.clone();
			let cloned_end_handler = self.on_stream_end.clone();
			let cloned_nagle_guard = self.nagle_guard.clone();
			let cloned_handler = self.initial_service.clone();
			let cloned_state = self.state.clone();
			let copied_pre_nagle_hook = self.pre_nagle_hook;
			let copied_post_nagle_hook = self.post_nagle_hook;
			let copied_chunk_on_size = self.chunk_output_at_size;
			let copied_service_name = self.service_name;
			let copied_slowloris_timeout = self.slowloris_timeout;
			let stream_id = STREAM_ID.fetch_add(1, Ordering::SeqCst);
			let copied_slowdown = self.cat_dev_slowdown;
			#[cfg(debug_assertions)]
			let trace_io = self.trace_during_debug;

			TaskBuilder::new()
				.name("cat_dev::net::tcp_server::bind().connection.handle")
				.spawn(async move {
					if let Err(cause) = Self::handle_tcp_connection(
						cloned_begin_handler,
						cloned_end_handler,
						cloned_nagle_guard,
						copied_slowloris_timeout,
						cloned_handler,
						stream,
						client_address,
						copied_pre_nagle_hook,
						copied_post_nagle_hook,
						copied_chunk_on_size,
						cloned_state,
						stream_id,
						copied_slowdown,
						#[cfg(debug_assertions)]
						trace_io,
					)
					.instrument(error_span!(
						"CatDevTCPServerAccept",
						client.address = %client_address,
						server.address = %loggable_address,
						server.service = copied_service_name,
						server.stream_id = stream_id,
					))
					.await
					{
						warn!(
							?cause,
							client.address = %client_address,
							server.address = %loggable_address,
							server.service = %copied_service_name,
							"Error escaped while handling TCP connection.",
						);
					}
				})
				.map_err(CatBridgeError::SpawnFailure)?;
		}
	}

	/// Attempt to handle a TCP connection, or "stream".
	///
	/// This will be called immediately from the task that spawns on
	/// connections. It will automatically spin down "shortly" after
	/// the connection is gone. We basically wait for an error back from the OS.
	///
	/// ## Errors
	///
	/// If reading, or writing to the underlying TCP Stream runs into any errors.
	/// Individual packet errors do not necissarily correlate to a closed
	/// connection. However, most services will link the two together.
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
	async fn handle_tcp_connection(
		on_stream_begin_handler: Option<UnderlyingOnStreamBeginService<State>>,
		on_stream_end_handler: Option<UnderlyingOnStreamEndService<State>>,
		nagle_guard: NagleGuard,
		slowloris_timeout: Duration,
		handler: BoxCloneService<Request<State>, Response, Infallible>,
		mut tcp_stream: TcpStream,
		client_address: SocketAddr,
		pre_hook_cloned: Option<&'static dyn PreNagleFnTy>,
		post_hook_cloned: Option<&'static dyn PostNagleFnTy>,
		chunk_output_at_size: Option<usize>,
		state: State,
		stream_id: u64,
		cat_dev_slowdown: Option<Duration>,
		#[cfg(debug_assertions)] trace_io: bool,
	) -> Result<(), CatBridgeError> {
		let (mut send_responses, mut packets_left_to_send) =
			bounded_channel::<ResponseStreamMessage>(128);

		// Run connection handlers and let them tell us if they want to allow this
		// stream setup to continue.
		if Self::initialize_stream(
			on_stream_begin_handler,
			&mut send_responses,
			&client_address,
			&state,
			&mut tcp_stream,
			stream_id,
		)
		.await?
		{
			return Ok(());
		}

		// Connection has been "approved", setup the on disconnect handler.
		let _guard = on_stream_end_handler.map(|service| {
			DisconnectAsyncDropServer::new(service, state.clone(), client_address, stream_id)
		});

		// Any previously saved data that was a victim of NAGLE's algorithim, or
		// similar.
		let mut nagle_cache: Option<(BytesMut, SystemTime)> = None;

		loop {
			let mut buff = BytesMut::with_capacity(TCP_READ_BUFFER_SIZE);
			tokio::select! {
				received = packets_left_to_send.recv() => {
					if Self::handle_server_write_to_connection(
						&mut tcp_stream,
						chunk_output_at_size,
						received,
						post_hook_cloned,
						stream_id,
						cat_dev_slowdown,
						#[cfg(debug_assertions)] trace_io,
					).await? {
						break;
					}
				}
				res_size = tcp_stream.read_buf(&mut buff) => {
					let size = res_size.map_err(NetworkError::IO)?;
					buff.truncate(size);
					if buff.is_empty() {
						continue;
					}

					let (should_break, returned_stream) = Self::handle_server_read_from_connection(
						tcp_stream,
						buff,
						send_responses.clone(),
						&nagle_guard,
						slowloris_timeout,
						handler.clone(),
						&mut nagle_cache,
						client_address,
						pre_hook_cloned,
						state.clone(),
						stream_id,
						#[cfg(debug_assertions)] trace_io,
					).await?;
					tcp_stream = returned_stream;
					if should_break {
						break;
					}
				}
			}
		}

		OUT_OF_BAND_SENDERS.remove_async(&stream_id).await;
		packets_left_to_send.close();
		std::mem::drop(tcp_stream.shutdown().await);

		Ok(())
	}

	async fn initialize_stream(
		on_stream_begin_handler: Option<UnderlyingOnStreamBeginService<State>>,
		send_channel: &mut BoundedSender<ResponseStreamMessage>,
		source_address: &SocketAddr,
		state: &State,
		tcp_stream: &mut TcpStream,
		stream_id: u64,
	) -> Result<bool, CatBridgeError> {
		tcp_stream.set_nodelay(true).map_err(NetworkError::IO)?;
		OUT_OF_BAND_SENDERS
			.upsert_async(stream_id, send_channel.clone())
			.await;

		if let Some(mut handle) = on_stream_begin_handler {
			if !handle
				.call(ResponseStreamEvent::new_with_state(
					send_channel.clone(),
					*source_address,
					Some(stream_id),
					state.clone(),
				))
				.await?
			{
				trace!("handler failed on stream begin hook");
				return Ok(true);
			}
		}

		Ok(false)
	}

	async fn handle_server_write_to_connection(
		tcp_stream: &mut TcpStream,
		chunk_output_on_size: Option<usize>,
		to_send_to_client_opt: Option<ResponseStreamMessage>,
		post_hook: Option<&'static dyn PostNagleFnTy>,
		stream_id: u64,
		cat_dev_slowdown: Option<Duration>,
		#[cfg(debug_assertions)] trace_io: bool,
	) -> Result<bool, CatBridgeError> {
		let Some(to_send_to_client) = to_send_to_client_opt else {
			return Ok(false);
		};

		match to_send_to_client {
			ResponseStreamMessage::Disconnect => {
				debug!("stream-disconnect-message");
				Ok(true)
			}
			ResponseStreamMessage::Response(resp) => {
				if let Some(body) = resp.body() {
					if !body.is_empty() {
						let messages = if let Some(size) = chunk_output_on_size {
							body.chunks(size)
								.map(Bytes::copy_from_slice)
								.collect::<Vec<_>>()
						} else {
							vec![body.clone()]
						};

						for message in messages {
							#[cfg(debug_assertions)]
							if trace_io {
								debug!(
									body.hex = format!("{message:02x?}"),
									body.str = String::from_utf8_lossy(&message).to_string(),
									"cat-dev-trace-output-tcp-server",
								);
							}

							let mut full_response = message.clone();
							if let Some(post) = post_hook {
								full_response = block_in_place(|| post(stream_id, full_response));
							}
							if let Some(slowdown_ms) = cat_dev_slowdown {
								sleep(slowdown_ms).await;
							}

							tcp_stream.writable().await.map_err(NetworkError::IO)?;
							tcp_stream
								.write_all(&full_response)
								.await
								.map_err(NetworkError::IO)?;
						}
					}
				}

				if resp.request_connection_close() {
					trace!("response-requested-connection-close");
					Ok(true)
				} else {
					Ok(false)
				}
			}
		}
	}

	#[allow(
		// All of our types are very differently typed, and well named, so chance
		// of confusion is low.
		//
		// Not to mention this is an internal only method.
		clippy::too_many_arguments,
	)]
	async fn handle_server_read_from_connection<'data>(
		mut stream: TcpStream,
		mut buff: BytesMut,
		channel: BoundedSender<ResponseStreamMessage>,
		nagle_guard: &'data NagleGuard,
		slowloris_timeout: Duration,
		mut handler: BoxCloneService<Request<State>, Response, Infallible>,
		nagle_cache: &'data mut Option<(BytesMut, SystemTime)>,
		client_address: SocketAddr,
		cloned_pre_nagle: Option<&'static dyn PreNagleFnTy>,
		state: State,
		stream_id: u64,
		#[cfg(debug_assertions)] trace_io: bool,
	) -> Result<(bool, TcpStream), CatBridgeError> {
		if let Some(convert_fn) = cloned_pre_nagle {
			block_in_place(|| {
				(*convert_fn)(stream_id, &mut buff);
			});
		}

		#[cfg(debug_assertions)]
		{
			if trace_io {
				debug!(
					body.hex = format!("{:02x?}", buff),
					body.str = String::from_utf8_lossy(&buff).to_string(),
					"cat-dev-trace-input-tcp-server",
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
				return Ok((true, stream));
			}

			existing_buff.extend(buff.freeze());
			buff = existing_buff;
		}

		let lockable_stream = Arc::new(Mutex::new(Some(stream)));
		while let Some((start_of_packet, end_of_packet)) = nagle_guard.split(&buff)? {
			let remaining_buff = buff.split_off(end_of_packet);
			let _start_of_buff = buff.split_to(start_of_packet);
			let req_body = buff.freeze();
			buff = remaining_buff;

			let mut request_object = Request::new_with_state_and_stream(
				req_body,
				client_address,
				state.clone(),
				Some(stream_id),
				lockable_stream.clone(),
			);
			request_object.extensions_mut().insert(channel.clone());
			if let Err(cause) = match handler.call(request_object).await {
				Ok(ref resp) => {
					channel
						.send(ResponseStreamMessage::Response(resp.clone()))
						.await
				}
				Err(cause) => {
					warn!(?cause, "request handler failed, will close connection.");
					channel.send(ResponseStreamMessage::Disconnect).await
				}
			} {
				warn!(
					?cause,
					"internal queue failure will not send disconnect/response."
				);
			}
		}
		{
			let mut done_lock = lockable_stream.lock().await;
			if let Some(strm) = done_lock.take() {
				stream = strm;
			} else {
				return Err(CommonNetNetworkError::StreamNoLongerProcessing.into());
			}
		}

		if !buff.is_empty() {
			_ = nagle_cache.insert((buff, start_time));
		}

		Ok((false, stream))
	}
}

impl<State: Clone + Debug + Send + Sync + 'static> Debug for TCPServer<State> {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		let mut dbg_struct = fmt.debug_struct("TCPServer");
		dbg_struct
			.field(
				"address_to_bind_or_connect_to",
				&self.address_to_bind_or_connect_to,
			)
			.field("cat_dev_slowdown", &self.cat_dev_slowdown)
			.field("chunk_output_at_size", &self.chunk_output_at_size)
			.field("initial_service", &self.initial_service)
			.field("nagle_guard", &self.nagle_guard)
			.field("on_stream_begin", &self.on_stream_begin)
			.field("on_stream_end", &self.on_stream_end)
			.field("has_pre_nagle_hook", &self.pre_nagle_hook.is_some())
			.field("has_post_nagle_hook", &self.post_nagle_hook.is_some())
			.field("service_name", &self.service_name)
			.field("slowloris_timeout", &self.slowloris_timeout)
			.field("state", &self.state);

		#[cfg(debug_assertions)]
		{
			dbg_struct.field("trace_during_debug", &self.trace_during_debug);
		}

		dbg_struct.finish()
	}
}

const TCP_SERVER_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("address_to_bind_or_connect_to"),
	NamedField::new("cat_dev_slowdown"),
	NamedField::new("chunk_output_at_size"),
	NamedField::new("initial_service"),
	NamedField::new("nagle_guard"),
	NamedField::new("on_stream_begin"),
	NamedField::new("on_stream_end"),
	NamedField::new("has_pre_nagle_hook"),
	NamedField::new("has_post_nagle_hook"),
	NamedField::new("service_name"),
	NamedField::new("slowloris_timeout"),
	NamedField::new("state"),
	#[cfg(debug_assertions)]
	NamedField::new("trace_during_debug"),
];

impl<State: Clone + Debug + Send + Sync + Valuable + 'static> Structable for TCPServer<State> {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static("TcpServer", Fields::Named(TCP_SERVER_FIELDS))
	}
}

impl<State: Clone + Debug + Send + Sync + Valuable + 'static> Valuable for TCPServer<State> {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			TCP_SERVER_FIELDS,
			&[
				Valuable::as_value(&format!("{}", self.address_to_bind_or_connect_to)),
				Valuable::as_value(&if let Some(slowdown) = self.cat_dev_slowdown {
					format!("{}ms", slowdown.as_millis())
				} else {
					"<none>".to_string()
				}),
				Valuable::as_value(&self.chunk_output_at_size),
				Valuable::as_value(&format!("{:?}", self.initial_service)),
				Valuable::as_value(&self.nagle_guard),
				Valuable::as_value(&format!("{:?}", self.on_stream_begin)),
				Valuable::as_value(&format!("{:?}", self.on_stream_end)),
				Valuable::as_value(&self.pre_nagle_hook.is_some()),
				Valuable::as_value(&self.post_nagle_hook.is_some()),
				Valuable::as_value(&self.service_name),
				Valuable::as_value(&format!("{:?}", self.slowloris_timeout)),
				Valuable::as_value(&self.state),
				#[cfg(debug_assertions)]
				Valuable::as_value(&self.trace_during_debug),
			],
		));
	}
}

#[cfg(test)]
pub mod test_helpers {
	use super::*;
	use std::net::{Ipv4Addr, SocketAddrV4};

	/// Get a free TCP Port for IPv4.
	///
	/// This will attempt to get a free tcp port on IPv4 by actually binding to
	/// port `0` which should make the OS automatically assign a free unused
	/// port.
	pub async fn get_free_tcp_v4_port() -> Option<u16> {
		let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
		if let Ok(bound) = TcpListener::bind(addr).await {
			if let Ok(local) = bound.local_addr() {
				return Some(local.port());
			}
		}
		None
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use crate::net::{
		CURRENT_TIME,
		server::{Router, requestable::Extension, test_helpers::*},
	};
	use bytes::Bytes;
	use std::{
		net::{Ipv4Addr, SocketAddrV4},
		sync::{
			Arc, Mutex,
			atomic::{AtomicU8, Ordering},
		},
		time::Duration,
	};
	use tokio::time::timeout;

	fn set_now(new_time: SystemTime) {
		CURRENT_TIME.with(|time_lazy| {
			*time_lazy.write().expect("RwLock is poisioned?") = new_time;
		})
	}

	#[tokio::test]
	pub async fn test_full_server() {
		let connected_fired = Arc::new(Mutex::new(false));
		let on_disconnect_fired = Arc::new(Mutex::new(false));
		let request_fired = Arc::new(Mutex::new(false));

		async fn on_connection(
			Extension(connected): Extension<Arc<Mutex<bool>>>,
		) -> Result<bool, CatBridgeError> {
			let mut locked = connected
				.lock()
				.expect("Failed to lock connected fired extension");
			*locked = true;
			Ok(true)
		}
		async fn on_disconnect(
			Extension(disconnected): Extension<Arc<Mutex<bool>>>,
		) -> Result<(), CatBridgeError> {
			let mut locked = disconnected
				.lock()
				.expect("Failed to lock connected fired extension");
			*locked = true;
			Ok(())
		}
		async fn on_request(
			Extension(request): Extension<Arc<Mutex<bool>>>,
		) -> Result<Response, CatBridgeError> {
			let mut locked = request
				.lock()
				.expect("Failed to lock connected fired extension");
			*locked = true;

			let mut resp = Response::new_with_body(Bytes::from(vec![0x1]));
			resp.should_close_connection();
			Ok(resp)
		}

		let mut router = Router::new();
		router
			.add_route(&[0x1, 0x2, 0x3], on_request)
			.expect("Failed to add a route!");
		router.layer(Extension(request_fired.clone()));

		let found_port = timeout(Duration::from_secs(5), get_free_tcp_v4_port())
			.await
			.expect("Timed out trying to find free port!")
			.expect("Failed to find free TCP port on system.");

		let mut srv = timeout(
			Duration::from_secs(5),
			TCPServer::new_with_state(
				"test",
				SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, found_port)),
				router,
				(None, None),
				NagleGuard::EndSigilSearch(&[0xFF, 0xFF, 0xFF, 0xFF]),
				(),
				#[cfg(debug_assertions)]
				true,
			),
		)
		.await
		.expect("Timed out starting server")
		.expect("Failed to create TCP Server.");

		srv.set_on_stream_begin(on_connection)
			.expect("Failed to register stream begin handler!");
		srv.layer_on_stream_begin(Extension(connected_fired.clone()))
			.expect("Failed to add layer to on stream begin!");
		srv.set_on_stream_end(on_disconnect)
			.expect("Failed to register stream end handler!");
		srv.layer_on_stream_end(Extension(on_disconnect_fired.clone()))
			.expect("Failed to add layer to on_disconnect!");

		let spawned =
			tokio::task::spawn(async move { srv.bind().await.expect("Failed to bind server!") });
		{
			loop {
				let client_stream_res = timeout(
					Duration::from_secs(10),
					TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, found_port)),
				)
				.await
				.expect("Service timed out waiting for connection!");
				// Service hasn't binded yet!
				if client_stream_res.is_err() {
					continue;
				}
				let mut client_stream = client_stream_res.unwrap();
				client_stream
					.write_all(&[0x1, 0x2, 0x3, 0xFF, 0xFF, 0xFF, 0xFF])
					.await
					.expect("Failed to write to client stream");
				// Wait til we get a response.
				let mut buff = [0_u8; 1];
				timeout(Duration::from_secs(5), client_stream.read(&mut buff))
					.await
					.expect("Timed out reading from client stream")
					.expect("Failed to read data from client stream");
				timeout(Duration::from_secs(5), client_stream.shutdown())
					.await
					.expect("Timed out shutting down client stream")
					.expect("Failed to shutdown client stream.");
				break;
			}
		}
		// Destruct the server.
		std::mem::drop(spawned);

		let locked_connect = connected_fired
			.lock()
			.expect("Failed to lock second connect");
		let locked_disconnect = on_disconnect_fired
			.lock()
			.expect("Failed to lock second on_disconnect");
		let locked_request = request_fired.lock().expect("Failed to lock second request");

		assert!(*locked_connect, "on connection handler never fired!");
		assert!(*locked_disconnect, "on disconnect handler never fired!");
		assert!(*locked_request, "on request handler never fired!");
	}

	#[tokio::test]
	pub async fn test_nagled() {
		let requests_fired = Arc::new(AtomicU8::new(0));

		async fn on_request(
			Extension(request): Extension<Arc<AtomicU8>>,
		) -> Result<Response, CatBridgeError> {
			request.fetch_add(1, Ordering::SeqCst);
			let resp = Response::new_with_body(Bytes::from(vec![0x1]));
			Ok(resp)
		}

		let mut router = Router::new();
		router
			.add_route(&[0x1, 0x2, 0x3], on_request)
			.expect("Failed to add a route!");
		router.layer(Extension(requests_fired.clone()));

		let found_port = timeout(Duration::from_secs(5), get_free_tcp_v4_port())
			.await
			.expect("Timed out finding port to bind too!")
			.expect("Failed to find any free tcp v4 port on system!");
		let srv = timeout(
			Duration::from_secs(5),
			TCPServer::new_with_state(
				"test",
				SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, found_port)),
				router,
				(None, None),
				NagleGuard::EndSigilSearch(&[0xFF, 0xFF, 0xFF, 0xFF]),
				(),
				#[cfg(debug_assertions)]
				true,
			),
		)
		.await
		.expect("timed out starting TCP Server for test")
		.expect("falied to create local tcp server!");

		let spawned =
			tokio::task::spawn(async move { srv.bind().await.expect("Failed to bind server!") });
		{
			loop {
				let client_stream_res = timeout(
					Duration::from_secs(10),
					TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, found_port)),
				)
				.await
				.expect("Service timed out waiting for connection!");
				// Service hasn't binded yet!
				if client_stream_res.is_err() {
					continue;
				}
				let mut client_stream = client_stream_res.unwrap();

				client_stream
					.write_all(&[0x1, 0x2, 0x3, 0xFF, 0xFF])
					.await
					.expect("Failed to write to client_stream");
				client_stream
					.flush()
					.await
					.expect("Failed to flush client_stream");
				client_stream
					.write_all(&[0xFF, 0xFF, 0x1, 0x2, 0x3, 0xFF, 0xFF, 0xFF, 0xFF])
					.await
					.expect("Failed to issue second write call to client_stream");
				let mut buff = [0_u8; 2];
				let read_bytes = timeout(Duration::from_secs(5), client_stream.read(&mut buff))
					.await
					.expect("Timed out reading from client_stream")
					.expect("Failed to read from client_stream!");
				if read_bytes == 1 {
					timeout(Duration::from_secs(5), client_stream.read(&mut buff[1..]))
						.await
						.expect("Timed out reading from client_stream")
						.expect("Failed to read from client_stream!");
				}

				timeout(Duration::from_secs(5), client_stream.shutdown())
					.await
					.expect("Timed out shutting down client stream")
					.expect("Failed to shutdown client stream.");
				break;
			}
		}
		// Shuts the server down.
		std::mem::drop(spawned);

		assert_eq!(
			requests_fired.load(Ordering::SeqCst),
			2,
			"on request did not fire the correct amount of times!",
		);
	}

	#[tokio::test]
	pub async fn test_slowloris_blocking() {
		let mut router = Router::new();
		router
			.add_route(&[0x1, 0x2, 0x3], || async {
				Ok(Response::new_with_body(Bytes::from(vec![0x1])))
			})
			.expect("Failed to add a route!");

		let found_port = timeout(Duration::from_secs(5), get_free_tcp_v4_port())
			.await
			.expect("Timed out finding port to bind too!")
			.expect("Failed to find any free tcp v4 port on system!");
		let srv = timeout(
			Duration::from_secs(5),
			TCPServer::new_with_state(
				"test",
				SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, found_port)),
				router,
				(None, None),
				NagleGuard::EndSigilSearch(&[0x10, 0x11, 0x12]),
				(),
				#[cfg(debug_assertions)]
				true,
			),
		)
		.await
		.expect("timed out starting TCP Server for test")
		.expect("falied to create local tcp server!");

		let spawned =
			tokio::task::spawn(async move { srv.bind().await.expect("Failed to bind server!") });
		let read_bytes;
		{
			loop {
				let client_stream_res = timeout(
					Duration::from_secs(10),
					TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, found_port)),
				)
				.await
				.expect("Service timed out waiting for connection!");
				// Service hasn't binded yet!
				if client_stream_res.is_err() {
					continue;
				}
				let mut client_stream = client_stream_res.unwrap();

				client_stream
					.write_all(&[0x1, 0x2, 0x3, 0x10])
					.await
					.expect("Failed to write to client_stream");
				client_stream
					.flush()
					.await
					.expect("Failed to flush client_stream");
				// This ensures the server running in the background has for sure
				// cached the time we started processing.
				tokio::time::sleep(Duration::from_secs(5)).await;
				set_now(
					SystemTime::now()
						.checked_add(Duration::from_secs(900_00_000))
						.expect("Failed to add time to systemtime"),
				);
				// We do properly finish off the packet but it's too late...
				client_stream
					.write_all(&[0x11, 0x12])
					.await
					.expect("Failed to write to client_stream");

				let mut buff = [0_u8; 1];
				// this read should timeout as the server will close our connection, so we shouldn't hit
				// the timeout.
				read_bytes = timeout(Duration::from_secs(10), client_stream.read(&mut buff))
					.await
					.expect("timed out trying to wait for disconnect")
					.expect("failure reading from stream");
				break;
			}
		}
		std::mem::drop(spawned);

		assert_eq!(read_bytes, 0, "Client didn't error on slowloris'd packet");
	}
}
