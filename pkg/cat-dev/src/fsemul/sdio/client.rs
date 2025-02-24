//! "Client" implementations for SDIO protocols for handling PCFS for cat-dev.
//!
//! These are ironically called "Servers", because the server actually ends up
//! connecting to the client for SDIO. Which is kind of weird, but it's what
//! happens.
//!
//! I promise servers are meant to be in the clients feature flag.

use crate::{
	errors::{APIError, CatBridgeError, NetworkError},
	fsemul::sdio::{
		DEFAULT_SDIO_BLOCK_PORT, DEFAULT_SDIO_CONTROL_PORT, SDIO_TCP_PACKET_BUFFER_SIZE,
		errors::SDIOAPIError,
		proto::{SdioControlReadRequest, SdioControlWriteRequest},
	},
};
use bytes::{Bytes, BytesMut};
use fnv::FnvHashMap;
use futures::future::join_all;
use local_ip_address::local_ip;
use std::{
	hash::BuildHasherDefault,
	net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4},
	sync::Arc,
	time::Duration,
};
use tokio::{
	io::{AsyncReadExt, AsyncWriteExt},
	net::{TcpListener, TcpStream},
	sync::{
		Mutex, RwLock, RwLockReadGuard,
		mpsc::{Receiver, Sender, channel as bounded_channel},
	},
	task::{Builder as TaskBuilder, JoinHandle},
	time::sleep,
};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error_span, warn};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// Create a server that someone connect to, that will allow us to read raw
/// SDIO addresses.
///
/// "SDIO" is actually handled by a pair of ports, and isn't real SDIO that's
/// happening over like a real chip. So if you came hear searching for real
/// SDIO code, I'm sorry. Nintendo calls this SDIO, and does it over a pair
/// of TCP ports. You came to the wrong place.
///
/// The two ports that make up an SDIO Client are the "control" port, where
/// clients can send "printf" debug logs, as well as requests to read/write
/// data on the other stream. The other port called the "data" port is where
/// the underlying data is actually sent from one side to another.
#[derive(Debug)]
pub struct SdioServer {
	/// A list of the active connections that you can send data to.
	active_connections: Arc<RwLock<Vec<SdioServerConnection>>>,
	/// The listener for "BLOCK", or data connections.
	block: Option<TcpListener>,
	/// The listener for "CONTROL" connections.
	control: Option<TcpListener>,
	/// Signal to trigger shutdown of the server, and stop the shutdown.
	on_shutdown: CancellationToken,
}

impl SdioServer {
	/// Construct a new SDIO client that listens, and waits for connections as a
	/// server would.
	///
	/// ## Errors
	///
	/// - If you did not pass in an explicit ip address to be bound upon, and we
	///   could not look up your ip!
	/// - If you we could not bind to the address we want to listen onto.
	pub async fn new(
		address: Option<Ipv4Addr>,
		control_port: Option<u16>,
		block_port: Option<u16>,
	) -> Result<Self, CatBridgeError> {
		let Some(ip) = address.or_else(|| {
			// This always returns an ipv4 address, but is still returning
			// an ip address for legacy reasons.
			local_ip().ok().map(|ip| match ip {
				IpAddr::V4(v4) => v4,
				IpAddr::V6(_v6) => unreachable!(),
			})
		}) else {
			return Err(APIError::NoHostIpFound.into());
		};

		let bound_ctrl_address =
			SocketAddrV4::new(ip, control_port.unwrap_or(DEFAULT_SDIO_CONTROL_PORT));
		let ctrl_server = TcpListener::bind(bound_ctrl_address)
			.await
			.map_err(NetworkError::IO)?;

		let bound_block_address =
			SocketAddrV4::new(ip, block_port.unwrap_or(DEFAULT_SDIO_BLOCK_PORT));
		let block_server = TcpListener::bind(bound_block_address)
			.await
			.map_err(NetworkError::IO)?;

		Ok(Self {
			active_connections: Arc::new(RwLock::new(Vec::with_capacity(0))),
			block: Some(block_server),
			control: Some(ctrl_server),
			on_shutdown: CancellationToken::new(),
		})
	}

	/// Get the list of connections actively connected to this server.
	pub async fn list_connections(&self) -> RwLockReadGuard<'_, Vec<SdioServerConnection>> {
		self.active_connections.read().await
	}

	/// Attempt to send a raw write request to all active connections.
	///
	/// Inner connections may fail and timeout, but all results will be attempt
	/// to be written to, and receive all the data at once.
	pub async fn read_all(
		&self,
		request: SdioControlReadRequest,
	) -> FnvHashMap<(SocketAddr, SocketAddr), Result<Bytes, NetworkError>> {
		let active = self.active_connections.read().await;

		// Try to receive across all data tasks all at once.
		let mut handles = Vec::with_capacity(active.len());
		for conn in active.iter() {
			handles.push(conn.read(request.clone()));
		}
		// `write` is guaranteed to exit soon as paart of it's own internal
		// routines.
		Self::join_all_inner_timeout(&active, handles).await
	}

	/// Attempt to send a raw write request to all active connections.
	///
	/// Inner connections may fail and timeout, but all results will be attempt
	/// to be written to, and receive all the data at once.
	pub async fn read_raw_all(
		&self,
		request: SdioControlReadRequest,
	) -> FnvHashMap<(SocketAddr, SocketAddr), Result<(Option<Bytes>, Option<Bytes>), NetworkError>>
	{
		let active = self.active_connections.read().await;

		// Try to receive across all data tasks all at once.
		let mut handles = Vec::with_capacity(active.len());
		for conn in active.iter() {
			handles.push(conn.read_raw(request.clone()));
		}
		// `write_raw` is guaranteed to exit soon as paart of it's own internal
		// routines.
		Self::join_all_inner_timeout(&active, handles).await
	}

	/// Attempt to send a raw write request to all active connections.
	///
	/// Inner connections may fail and timeout, but all results will be attempt
	/// to be written to, and receive all the data at once.
	pub async fn write_all(
		&self,
		request: SdioControlWriteRequest,
		data_to_write: Bytes,
	) -> FnvHashMap<(SocketAddr, SocketAddr), Result<Bytes, NetworkError>> {
		let active = self.active_connections.read().await;

		// Try to receive across all data tasks all at once.
		let mut handles = Vec::with_capacity(active.len());
		for conn in active.iter() {
			handles.push(conn.write(request.clone(), data_to_write.clone()));
		}
		// `write` is guaranteed to exit soon as paart of it's own internal
		// routines.
		Self::join_all_inner_timeout(&active, handles).await
	}

	/// Attempt to send a raw write request to all active connections.
	///
	/// Inner connections may fail and timeout, but all results will be attempt
	/// to be written to, and receive all the data at once.
	pub async fn write_raw_all(
		&self,
		request: SdioControlWriteRequest,
		data_to_write: Bytes,
	) -> FnvHashMap<(SocketAddr, SocketAddr), Result<(Option<Bytes>, Option<Bytes>), NetworkError>>
	{
		let active = self.active_connections.read().await;

		// Try to receive across all data tasks all at once.
		let mut handles = Vec::with_capacity(active.len());
		for conn in active.iter() {
			handles.push(conn.write_raw(request.clone(), data_to_write.clone()));
		}
		// `write_raw` is guaranteed to exit soon as paart of it's own internal
		// routines.
		Self::join_all_inner_timeout(&active, handles).await
	}

	/// Try to receive data from all the connection at once.
	///
	/// This will simply attempt to read from all the active connections at a single
	/// time.
	pub async fn try_recv_all(
		&self,
	) -> FnvHashMap<(SocketAddr, SocketAddr), (Option<Bytes>, Option<Bytes>)> {
		let active = self.active_connections.read().await;

		// Try to receive across all data tasks all at once.
		let mut handles = Vec::with_capacity(active.len());
		for conn in active.iter() {
			handles.push(conn.try_recv());
		}
		// `try_recv` is guaranteed to exit soon as paart of it's own internal
		// routines.
		Self::join_all_inner_timeout(&active, handles).await
	}

	/// Start serving this current client.
	///
	/// This should only ever be called once when yo uwant to start
	/// accepting connections.
	///
	/// ## Errors
	///
	/// If serve has already been called, or we could not spawn the task to
	/// handle serving the client.
	pub fn serve(&mut self) -> Result<JoinHandle<()>, CatBridgeError> {
		let Some(taken_ctrl) = self.control.take() else {
			return Err(SDIOAPIError::CannotServeServerTwice.into());
		};
		let Some(taken_block) = self.block.take() else {
			return Err(SDIOAPIError::CannotServeServerTwice.into());
		};
		let cloned_shutdown = self.on_shutdown.clone();

		let ctrl_formatted = format!(
			"{}",
			taken_ctrl
				.local_addr()
				.unwrap_or_else(|_| { SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)) }),
		);
		let block_formatted = format!(
			"{}",
			taken_block
				.local_addr()
				.unwrap_or_else(|_| { SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)) }),
		);
		let connection_list = self.active_connections.clone();

		let task_result = TaskBuilder::new()
			.name("cat_dev::fsemul::sdio_server::serve")
			.spawn(
				async move {
					Self::do_serve(connection_list, taken_ctrl, taken_block, cloned_shutdown).await;
				}
				.instrument(error_span!(
					"cat_dev::sdio_server::serving",
					sdio_client.ctrl_host_addr = ctrl_formatted,
					sdio_client.block_host_addr = block_formatted,
				)),
			);

		task_result.map_err(CatBridgeError::SpawnFailure)
	}

	/// Shutdown this server, and all associated tasks for connections/packets
	/// have been fully shutdown.
	pub fn shutdown(self) {
		self.on_shutdown.cancel();
	}

	/// Called within a background task, actually performs serving of files.
	///
	/// This function ccan't error in the main loop as it runs in the background,
	/// but we can and should log any errors so the user can know.
	async fn do_serve(
		connection_list: Arc<RwLock<Vec<SdioServerConnection>>>,
		ctrl: TcpListener,
		block: TcpListener,
		on_shutdown: CancellationToken,
	) {
		loop {
			let (ctrl_stream, ctrl_addr) = tokio::select! {
				res = ctrl.accept() => {
					match res {
						Ok(tuple) => tuple,
						Err(cause) => {
							warn!(
							  ?cause,
							  "Failed to accept next control connection, due to underlying error. Please try connecting again..",
							);
							continue;
						}
					}
				}
				() = on_shutdown.cancelled() => {
					break;
				}
			};

			debug!(
			  sdio_client.ctrl_client_addr = %ctrl_addr,
			  "Received SDIO CTRL connection, waiting for block connection...",
			);

			let (block_stream, block_addr) = tokio::select! {
				res  = block.accept() => {
					match res {
						Ok(tuple) => tuple,
						Err(cause) => {
							warn!(
								?cause,
							  sdio_client.ctrl_client_addr = %ctrl_addr,
								"Failed to accept next block connection, due to underlying error. Please try connecting again..",
							);
							continue;
						}
					}
				}
				() = on_shutdown.cancelled() => {
					break;
				}
			};

			debug!(
			  sdio_client.ctrl_client_addr = %ctrl_addr,
			  sdio_client.block_client_addr = %block_addr,
			  "Received SDIO Client connection.... serving...",
			);

			let cloned_list = connection_list.clone();
			let cloned_cancellation_token = on_shutdown.clone();
			if let Err(cause) = TaskBuilder::new()
				.name("cat_dev::fsemul::sdio_server::serve_connection")
				.spawn(
					async move {
						Self::serve_connection(
							cloned_list,
							(ctrl_addr, ctrl_stream),
							(block_addr, block_stream),
							cloned_cancellation_token,
						)
						.await;
					}
					.instrument(error_span!(
						"cat_dev::fsemul::sdio::serve_connection",
					  sdio_client.ctrl_client_addr = %ctrl_addr,
					  sdio_client.block_client_addr = %block_addr,
					)),
				) {
				warn!(
				  ?cause,
				  sdio_client.ctrl_client_addr = %ctrl_addr,
				  sdio_client.block_client_addr = %block_addr,
				  "Failed to spawn handler for SDIO Connection, please reconnect...",
				);
			}
		}
	}

	/// Serve a particular connection for an SDIO server that has connected to
	/// us.
	///
	/// Much like `do_serve` this also runs in the background, and as a result
	/// you must log all errors manually.
	async fn serve_connection(
		connection_list: Arc<RwLock<Vec<SdioServerConnection>>>,
		mut control: (SocketAddr, TcpStream),
		mut block: (SocketAddr, TcpStream),
		on_server_shutdown: CancellationToken,
	) {
		let (in_send, mut in_recv) =
			bounded_channel::<(Option<Bytes>, Option<Bytes>)>(SDIO_TCP_PACKET_BUFFER_SIZE);
		let (out_send, out_recv) =
			bounded_channel::<(Option<Bytes>, Option<Bytes>)>(SDIO_TCP_PACKET_BUFFER_SIZE);
		let cloned_cancellation_token: CancellationToken;
		let mut global_shutdown = false;

		// Allow people to interact with our actual connection.
		{
			let mut guard = connection_list.write().await;
			let connection = SdioServerConnection::new(control.0, block.0, in_send, out_recv);
			cloned_cancellation_token = connection.cloned_cancellation_token();
			guard.push(connection);
		}

		// Now let's actually process our connection.
		loop {
			let (control_out, data_out) = tokio::select! {
				opt = in_recv.recv() => {
					if let Some(tuple) = opt {
						tuple
					} else {
						continue;
					}
				}
				() = on_server_shutdown.cancelled() => {
					global_shutdown = true;
					break;
				}
				() = cloned_cancellation_token.cancelled() => {
					break;
				}
			};

			// Send data out when we get some...
			if let Some(to_ctrl) = control_out {
				if let Err(cause) = control.1.write_all(&to_ctrl).await {
					warn!(?cause, "Failed to send data to control!");
				}
			}

			if let Some(to_block) = data_out {
				if let Err(cause) = block.1.write_all(&to_block).await {
					warn!(?cause, "Failed to send data to block!");
				}
			}

			// And wait for a response back potentially...
			let control_resp = Self::try_recv_data(&mut control.1).await;
			let block_resp = Self::try_recv_data(&mut block.1).await;
			if let Err(cause) = out_send.send((control_resp, block_resp)).await {
				warn!(
					?cause,
					"Failed to communicate response to other parts of the program",
				);
			}
		}

		// Remove ourselves from the list, unless we are shutting everything down
		// which case the list will be fully removed anyway so ignore all that.
		if !global_shutdown {
			{
				let mut guard = connection_list.write().await;
				if let Some(pos) = guard.iter().position(|sock| sock == (control.0, block.0)) {
					guard.remove(pos);
				}
			}
		}
	}

	/// This will attempt to try and read data off of the network until a
	/// response hasn't been hit for 100ms.
	///
	/// This is mostly a very hacky method to be utilized when we don't know if
	/// the remote side is sending us anything, but we do want to know if it sent
	/// _something_.
	///
	/// This is mostly used in the "Scientist" classes which attempt to diff our
	/// implementation with an official cafe-sdk implementation in real time,
	/// printing out any differences in sent/received data.
	///
	/// ## Errors
	///
	/// If the underlying stream returns an error for us.
	async fn try_recv_data(stream: &mut TcpStream) -> Option<Bytes> {
		let mut final_buff = BytesMut::with_capacity(8192);
		let mut buff = BytesMut::zeroed(8192);

		loop {
			tokio::select! {
			  res = stream.read(&mut buff) => {
					let Ok(read_bytes) = res.map_err(NetworkError::IO) else {
						break;
					};
					final_buff.extend(&buff[..read_bytes]);
			  }
			  () = sleep(Duration::from_millis(100)) => {
					break;
			  }
			}
		}

		if buff.is_empty() {
			None
		} else {
			Some(buff.freeze())
		}
	}

	/// Join all of the items that are in the order of active connections.
	///
	/// This will turn them into a map properly so that way
	async fn join_all_inner_timeout<IterTy, FutureTy>(
		active: &RwLockReadGuard<'_, Vec<SdioServerConnection>>,
		to_join: IterTy,
	) -> FnvHashMap<(SocketAddr, SocketAddr), FutureTy>
	where
		IterTy: IntoIterator,
		IterTy::Item: Future<Output = FutureTy>,
	{
		let mut joined = join_all(to_join).await;
		let mut final_result =
			FnvHashMap::with_capacity_and_hasher(active.len(), BuildHasherDefault::default());
		// Remove from back to front to not only ignore performance constraints of
		// shifting to the let, but also make sure that no indexes go out of whack.
		for (idx, conn) in active.iter().enumerate().rev() {
			final_result.insert(
				(conn.control_address, conn.block_address),
				joined.remove(idx),
			);
		}
		final_result
	}
}

impl Drop for SdioServer {
	fn drop(&mut self) {
		self.on_shutdown.cancel();
	}
}

/// A remote host that has actively connected to us, and we should send
/// requests to.
#[derive(Debug)]
pub struct SdioServerConnection {
	/// The client address connected to the block port.
	block_address: SocketAddr,
	/// The client address connected to the control port.
	control_address: SocketAddr,
	/// The channel that we use to actively send data to the server.
	///
	/// Stored as tuples of (Control Data Out, Block Data Out). We will always
	/// send data out of control first, and then block.
	data_out_channel: Sender<(Option<Bytes>, Option<Bytes>)>,
	/// The channel we use to receive responses from the server.
	///
	/// This is locked as only one person can wait on a consumer at a time.
	data_in_channel: Mutex<Receiver<(Option<Bytes>, Option<Bytes>)>>,
	/// When this particular connection has been dropped, but not the entire server.
	on_connection_shutdown: CancellationToken,
}

impl SdioServerConnection {
	/// Thin wrapper around a single sdio server connection.
	#[must_use]
	pub fn new(
		ctrl: SocketAddr,
		block: SocketAddr,
		send: Sender<(Option<Bytes>, Option<Bytes>)>,
		recv: Receiver<(Option<Bytes>, Option<Bytes>)>,
	) -> Self {
		Self {
			block_address: block,
			control_address: ctrl,
			data_out_channel: send,
			data_in_channel: Mutex::new(recv),
			on_connection_shutdown: CancellationToken::new(),
		}
	}

	/// Shut this particular connection down, and tell it to stop processing data.
	pub fn shutdown(&self) {
		self.on_connection_shutdown.cancel();
	}

	/// Perform a read request, and get the data.
	///
	/// Note there is also: [`Self::read_raw`] which returns all of the data
	/// potentially read, which is mostly used for scientist code.
	///
	/// ## Errors
	///
	/// If we run into some sort of network error sending or receiving the
	/// packet, or if the server responds with no data on the data channel.
	pub async fn read(&self, request: SdioControlReadRequest) -> Result<Bytes, NetworkError> {
		// We only care about data from the data channel, as that's all the
		// read request _should_ respond with.
		//
		// Let's just ignore any data from control stream if you want the full
		// data please use read raw.
		self.read_raw(request)
			.await?
			.1
			.ok_or(NetworkError::ExpectedData)
	}

	/// Perform a read request, and get any raw data back.
	///
	/// Note there is also: [`Self::read`] which returns just the data we should
	/// get back from read.
	///
	/// ## Errors
	///
	/// If we run into some sort of network error sending or receiving the
	/// packet.
	pub async fn read_raw(
		&self,
		request: SdioControlReadRequest,
	) -> Result<(Option<Bytes>, Option<Bytes>), NetworkError> {
		// We want to take the IN lock _before_ sending data, so we
		// can ensure the next bytes in _are ours_, and that no one
		// tries to read our response packet.
		let mut inc = self.data_in_channel.lock().await;
		self.data_out_channel
			.send((Some(Bytes::from(request)), None))
			.await
			.map_err(NetworkError::SendMultiQueueFailure)?;

		// The task that sends to data in has their own timeouts.
		//
		// But a task might be cancelled so we should have our own timeout.
		tokio::select! {
			opt = inc.recv() => opt,
			() = sleep(Duration::from_secs(30)) => {
				return Err(NetworkError::ExpectedData);
			}
		}
		.ok_or(NetworkError::ExpectedData)
	}

	/// Perform a write request to random data.
	///
	/// Note there is also: [`Self::write_raw`] which returns all of the data
	/// potentially read, which is mostly used for scientist code.
	///
	/// ## Errors
	///
	/// If we run into some sort of network error sending or receiving the
	/// packet, or if the server responds with no data on the data channel.
	pub async fn write(
		&self,
		request: SdioControlWriteRequest,
		data_to_write: Bytes,
	) -> Result<Bytes, NetworkError> {
		// We only care about data from the control channel, as that's all the
		// write request _should_ respond with.
		//
		// Let's just ignore any data from control stream if you want the full
		// data please use read raw.
		self.write_raw(request, data_to_write)
			.await?
			.0
			.ok_or(NetworkError::ExpectedData)
	}

	/// Write some data over the SDIO connection.
	///
	/// This will return the raw responses from both the control and data stream.
	/// Usually only used by scientists. If you want the exacted data please see
	/// [`Self::write`].
	///
	/// ## Errors
	///
	/// If we cannot send or receive data to the connected host.
	pub async fn write_raw(
		&self,
		request: SdioControlWriteRequest,
		data_to_write: Bytes,
	) -> Result<(Option<Bytes>, Option<Bytes>), NetworkError> {
		// We want to take the IN lock _before_ sending data, so we
		// can ensure the next bytes in _are ours_, and that no one
		// tries to read our response packet.
		let mut inc = self.data_in_channel.lock().await;

		self.data_out_channel
			.send((Some(Bytes::from(request)), Some(data_to_write)))
			.await
			.map_err(NetworkError::SendMultiQueueFailure)?;

		// The task that sends to data in has their own timeouts.
		//
		// But a task might be cancelled so we should have our own timeout.
		tokio::select! {
			opt = inc.recv() => opt,
			() = sleep(Duration::from_secs(30)) => {
				return Err(NetworkError::ExpectedData);
			}
		}
		.ok_or(NetworkError::ExpectedData)
	}

	/// Just attempt to do a receive from both channels to peek what's on those
	/// contents.
	pub async fn try_recv(&self) -> (Option<Bytes>, Option<Bytes>) {
		let mut inc = self.data_in_channel.lock().await;

		tokio::select! {
			opt = inc.recv() => opt.unwrap_or_default(),
			() = sleep(Duration::from_secs(30)) => {
				(None, None)
			}
		}
	}

	/// Get a copy of the cancellation token to wait on.
	#[must_use]
	fn cloned_cancellation_token(&self) -> CancellationToken {
		self.on_connection_shutdown.clone()
	}
}

impl PartialEq<(SocketAddr, SocketAddr)> for &SdioServerConnection {
	fn eq(&self, other: &(SocketAddr, SocketAddr)) -> bool {
		other.0 == self.control_address && other.1 == self.block_address
	}
}
impl PartialEq<(SocketAddr, SocketAddr)> for SdioServerConnection {
	fn eq(&self, other: &(SocketAddr, SocketAddr)) -> bool {
		other.0 == self.control_address && other.1 == self.block_address
	}
}

const SDIO_SERVER_CONNECTION_BODY_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("client_control_address"),
	NamedField::new("client_block_address"),
];

impl Structable for SdioServerConnection {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SdioServerConnection",
			Fields::Named(SDIO_SERVER_CONNECTION_BODY_FIELDS),
		)
	}
}

impl Valuable for SdioServerConnection {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SDIO_SERVER_CONNECTION_BODY_FIELDS,
			&[
				Valuable::as_value(&format!("{}", self.control_address)),
				Valuable::as_value(&format!("{}", self.block_address)),
			],
		));
	}
}
