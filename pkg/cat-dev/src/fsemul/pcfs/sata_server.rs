//! Server implementation for SATA Server.

use crate::{
	errors::{APIError, CatBridgeError, NetworkError},
	fsemul::{
		pcfs::sata_proto::{
			SataCapabilitiesFlags, SataGetInfoByQueryPacketBody, SataProtoChunker, SataRequest,
			SataRequestBody,
		},
		HostFilesystem,
	},
};
use bytes::Bytes;
use futures::{stream::SplitSink, SinkExt, StreamExt};
use local_ip_address::local_ip;
use std::{
	net::{IpAddr, Ipv4Addr, SocketAddrV4},
	sync::{atomic::AtomicUsize, Arc},
	time::Duration,
};
use tokio::{
	net::{TcpListener, TcpStream},
	sync::mpsc::{channel, Sender},
	task::Builder as TaskBuilder,
	time::sleep,
};
use tokio_util::codec::Framed;
use tracing::{debug, error, error_span, field::valuable, trace, Instrument};

/// The default port to use for hosting the SATA Server.
pub const DEFAULT_PCFS_OVER_SATA_PORT: u16 = 7500_u16;

/// The amount of TCP Packets that can be buffered per client.
///
/// *note: this is not the size of an individual packet, or packets, but is
/// just the amount of packets that can be queued.*
const PCFS_SATA_TCP_PACKET_BUFFER_SIZE: usize = 8192_usize;

/// An implementation of a PCFS server speaking the Sata over PCFS protocol.
#[allow(
	// Clippy this is not a state machine.
	clippy::struct_excessive_bools,
)]
#[derive(Debug)]
pub struct PCFSSataServer<'fs> {
	/// The address we're actively bound and listening on.
	bound_address: SocketAddrV4,
	/// If we should disable a required load bearing sleep necessary for our
	/// server to function.
	disable_load_bearing_sleep: bool,
	/// Disable actually removing files from the filesystem.
	disable_real_removal: bool,
	/// A pointer to our integration with the host filesystem.
	///
	/// This let's us read data from the cafe directory and otherwise.
	host_filesystem: &'fs HostFilesystem,
	/// The listener to serve traffic on.
	server: TcpListener,
	/// If our SATA server should support FFIO (fast file I/O).
	///
	/// I'm gonna be honest I don't know what the difference here is yet.
	should_support_ffio: bool,
	/// If our SATA server should support Combined Send+Recv.
	///
	/// Combined Send/Recv presumably allows us to send data, and receive some
	/// in the same paccket. However, I haven't fully reverse engineered the
	/// difference between these two.
	should_support_csr: bool,
}

impl<'fs> PCFSSataServer<'fs> {
	/// Create and bind a new PCFS Sata Server to a port on an address.
	///
	/// ## Errors
	///
	/// If we cannot bind to the request host ip address.
	#[allow(
		// I think the overhead of converting here isn't worth it, but maybe one
		// day it'd be good to change to two variant enums.
		clippy::fn_params_excessive_bools,
	)]
	pub async fn new(
		host_filesystem: &'fs HostFilesystem,
		address: Option<Ipv4Addr>,
		port: Option<u16>,
		disable_real_removal: bool,
		should_support_ffio: bool,
		should_support_csr: bool,
		disable_load_bearing_sleep: bool,
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

		let bound_address = SocketAddrV4::new(ip, port.unwrap_or(DEFAULT_PCFS_OVER_SATA_PORT));
		let server = TcpListener::bind(bound_address)
			.await
			.map_err(NetworkError::IO)?;

		Ok(Self {
			bound_address,
			disable_load_bearing_sleep,
			disable_real_removal,
			host_filesystem,
			server,
			should_support_ffio,
			should_support_csr,
		})
	}

	/// If the load bearing sleep required for real cat-dev's has been disabled.
	#[must_use]
	pub const fn load_bearing_sleep_disabled(&self) -> bool {
		self.disable_load_bearing_sleep
	}

	/// If our server supports FFIO.
	#[must_use]
	pub const fn supports_ffio(&self) -> bool {
		self.should_support_ffio
	}

	/// If our server supports combined send+recv.
	#[must_use]
	pub const fn supports_combined_send_and_recv(&self) -> bool {
		self.should_support_csr
	}

	/// Get the port that the sata server will use.
	#[must_use]
	pub const fn port(&self) -> u16 {
		self.bound_address.port()
	}

	/// Actually end up serving connections to clients.
	///
	/// This will continue serving for as long as it is able. Although it doesn't
	/// do this as 'effeciently' because it cannot server multiple clients
	/// concurrently itself.
	///
	/// In order to do this, we would need a `'static` lifetime'd host filesystem
	/// which this method works without even `'static` filesystems. If you do
	/// have a `'static` host filesystem, you should prefer to use the method
	/// [`PCFSSataServer::serve_concurrently`] to more effeciently serve many
	/// clients at once.
	pub async fn serve(self) {
		let host_filesystem = self.host_filesystem;

		loop {
			match self.server.accept().await {
				Ok((stream, address)) => {
					let client = address;
					let bound = self.bound_address;
					if let Err(cause) = stream.set_nodelay(true) {
						error!(
						  ?cause,
						  server.address = %bound,
						  client.address = %client,
						  "Failed to disable NAGLE on connection, disabling...",
						);
						continue;
					}

					let result = Self::serve_connection(
						host_filesystem,
						stream,
						self.disable_real_removal,
						self.should_support_ffio,
						self.should_support_csr,
						self.disable_load_bearing_sleep,
					)
					.instrument(error_span!(
					  "cat_dev::fsemul::pcfs::sata::serve_connection",
					  server.address = %bound,
					  client.address = %client,
					))
					.await;

					if let Err(cause) = result {
						error!(
						  ?cause,
						  server.address = %bound,
						  client.address = %client,
						  "Failed to actually handle packets from client SATA connection.",
						);
					}
				}
				Err(cause) => {
					error!(
					  ?cause,
					  server.address = %self.bound_address,
					  "Failed to accept PCFS SATA Connection from client, cannot serve.",
					);
				}
			}
		}
	}

	/// Serve a connection non-concurrently (E.g. when not dealing with a static
	/// filesystem).
	#[allow(
		// This is okay as most of the logic outside of this function.
		clippy::too_many_lines,
		// I think the overhead of converting here isn't worth it, but maybe one
		// day it'd be good to change to two variant enums.
		clippy::fn_params_excessive_bools,
	)]
	async fn serve_connection(
		host_filesystem: &'fs HostFilesystem,
		connection: TcpStream,
		disable_real_removal: bool,
		mut supports_ffio: bool,
		mut supports_csr: bool,
		disable_load_bearing_sleep: bool,
	) -> Result<(), CatBridgeError> {
		connection.set_nodelay(true).map_err(NetworkError::IO)?;
		let bypass_buff_to_read = Arc::new(AtomicUsize::new(0));
		let (sink, mut stream) =
			Framed::new(connection, SataProtoChunker(bypass_buff_to_read.clone())).split();
		let mut first_packet = true;

		let sender = Self::spawn_write_task(disable_load_bearing_sleep, sink)?;

		loop {
			while let Some(result) = stream.next().await {
				let packet = result.map_err(NetworkError::IO)?.freeze();
				let parsed_packet = SataRequest::try_from(packet)?;
				parsed_packet.header().ensure_not_from_host()?;
				if first_packet {
					let flags = SataCapabilitiesFlags(parsed_packet.header().flags());
					if !flags.intersects(SataCapabilitiesFlags::FAST_FILE_IO_SUPPORTED) {
						debug!(
							flags = valuable(&flags),
							"Disabling FFIO because first packet header requested it..."
						);
						supports_ffio = false;
					}
					if !flags.intersects(SataCapabilitiesFlags::COMBINED_SEND_RECV_SUPPORTED) {
						debug!(
							flags = valuable(&flags),
							"Disabling CSR because first packet header requested it..."
						);
						supports_csr = false;
					}
				}
				first_packet = false;

				debug!(
					packet = valuable(&parsed_packet),
					"{}",
					parsed_packet.command_info(),
				);
				match parsed_packet.body() {
					SataRequestBody::ChangeMode(ref mode) => {
						sender
							.send(mode.handle(parsed_packet.header(), host_filesystem)?)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::ChangeOwner(ref co) => {
						sender
							.send(co.handle(parsed_packet.header())?)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::CloseFile(ref cf) => {
						sender
							.send(cf.handle(parsed_packet.header(), host_filesystem).await?)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::CloseFolder(ref cf) => {
						sender
							.send(cf.handle(parsed_packet.header(), host_filesystem).await?)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::CreateDirectory(ref cd) => {
						sender
							.send(cd.handle(parsed_packet.header(), host_filesystem).await?)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::GetInfoByQuery(ref info) => {
						sender
							.send(info.handle(parsed_packet.header(), host_filesystem).await?)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::OpenFile(ref file) => {
						sender
							.send(
								file.handle(
									parsed_packet.header(),
									parsed_packet.command_info(),
									host_filesystem,
								)
								.await?,
							)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::OpenFolder(ref folder) => {
						sender
							.send(
								folder
									.handle(parsed_packet.header(), host_filesystem)
									.await?,
							)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::Ping(ref ping) => {
						debug!(
							client.packet.header = valuable(parsed_packet.header()),
							client.packet.command_info = valuable(parsed_packet.command_info()),
							client.packet.body = valuable(ping),
							"received ping packet from client",
						);

						sender
							.send(ping.handle(
								parsed_packet.header(),
								parsed_packet.command_info(),
								supports_ffio,
								supports_csr,
							)?)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::ReadFile(ref rfr) => {
						rfr.handle(
							parsed_packet.header(),
							host_filesystem,
							supports_ffio,
							&sender,
						)
						.await?;
					}
					SataRequestBody::ReadDirectory(ref rd) => {
						sender
							.send(rd.handle(parsed_packet.header(), host_filesystem).await?)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::Remove(ref rm) => {
						sender
							.send(
								rm.handle(
									parsed_packet.header(),
									!disable_real_removal,
									host_filesystem,
								)
								.await?,
							)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::Rewind(ref rewind) => {
						sender
							.send(
								rewind
									.handle(parsed_packet.header(), host_filesystem)
									.await?,
							)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::StatFile(ref st) => {
						sender
							.send(
								SataGetInfoByQueryPacketBody::stat_fd(
									parsed_packet.header(),
									host_filesystem,
									st.file_descriptor(),
								)
								.await?,
							)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::WriteFile(ref wf) => {
						sender
							.send(
								wf.handle(
									parsed_packet.header(),
									host_filesystem,
									supports_ffio,
									&mut stream,
									&bypass_buff_to_read,
								)
								.await?,
							)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
				}
			}
		}
	}

	/// Spawn a task that will watch a channel, and send data out over a socket,
	/// sleeping inbetween to prevent the cat-dev from processing incorrectly.
	///
	/// Do this because the default cat-dev tcp stack is a giant piece of shit,
	/// who i want to throw into the biggest fire. Whoever decided sending an
	/// ACK before we were ready has forever ruined my life. I just wanna talk
	/// with them.
	fn spawn_write_task(
		disable_load_bearing_sleep: bool,
		mut sink: SplitSink<Framed<TcpStream, SataProtoChunker>, Bytes>,
	) -> Result<Sender<Bytes>, CatBridgeError> {
		let (sender, mut receiver) = channel::<Bytes>(PCFS_SATA_TCP_PACKET_BUFFER_SIZE);

		TaskBuilder::new()
			.name("cat_dev::fsemul::pcfs::sata::write_task")
			.spawn(async move {
				while let Some(packet) = receiver.recv().await {
					if let Err(cause) = sink.send(packet).await {
						error!(
							?cause,
							"Failed to send packet over PCFS Sata, error in write channel, shutting down write task for client",
						);
						break;
					}

					if !disable_load_bearing_sleep {
						// Yes, this is a very load bearing sleep.
						//
						// Without this sleep, the tcp client/server implementation on the
						// MION will ack packets before this ready, either getting into a
						// state where it thinks it needs to wait for more data, OR a state
						// where it just misreads data, and thinks we're doing things like
						// sending back a file fd of 0. Which is never true.
						//
						// A user can techincally turn this off, BUT you will notice spurious
						// errors on real cat-dev hardware.
						trace!("sleeping to work around MION TCP buffer-bug...");
						sleep(Duration::from_millis(25)).await;
					}
				}
			})
			.map_err(CatBridgeError::SpawnFailure)?;

		Ok(sender)
	}
}

impl PCFSSataServer<'static> {
	/// Actually end up serving connections to clients.
	///
	/// This will continue serving for as long as it is able. This is the
	/// significantly more efficient than just [`PCFSSataServer::serve`] as we can
	/// confirm that host filesystem will ive long enough for all of our
	/// connections.
	pub async fn serve_concurrently(self) {
		let host_filesystem: &'static HostFilesystem = self.host_filesystem;

		loop {
			match self.server.accept().await {
				Ok((stream, address)) => {
					let client = address;
					let bound = self.bound_address;
					let spawn_result = TaskBuilder::new()
						.name("cat_dev::fsemul::pcfs::sata::serve_client_connection_concurrently")
						.spawn(async move {
							let result = Self::serve_connection_concurrently(
								host_filesystem,
								stream,
								self.disable_real_removal,
								self.should_support_ffio,
								self.should_support_csr,
								self.disable_load_bearing_sleep,
							)
							.instrument(error_span!(
							  "cat_dev::fsemul::pcfs::sata::serve_connection_concurrently",
							  server.address = %bound,
							  client.address = %client,
							))
							.await;

							if let Err(cause) = result {
								error!(
								  ?cause,
								  server.address = %bound,
								  client.address = %client,
								  "Failed to actually handle packets from client connection",
								);
							}
						});

					if let Err(cause) = spawn_result {
						error!(
						  ?cause,
						  server.address = %self.bound_address,
						  client.address = %address,
						  "Failed to spawn handler for PCFS Sata Connection, cannot serve task.",
						);
					}
				}
				Err(cause) => {
					error!(
					  ?cause,
					  server.address = %self.bound_address,
					  "Failed to accept PCFS SATA Connection from client, cannot serve itself.",
					);
				}
			}
		}
	}

	#[allow(
		// This is okay as most of the logic outside of this function.
		clippy::too_many_lines,
		// Refactor one day maybe.
		clippy::fn_params_excessive_bools,
	)]
	async fn serve_connection_concurrently(
		host_filesystem: &'static HostFilesystem,
		connection: TcpStream,
		disable_real_removal: bool,
		mut supports_ffio: bool,
		mut supports_csr: bool,
		disable_load_bearing_sleep: bool,
	) -> Result<(), CatBridgeError> {
		connection.set_nodelay(true).map_err(NetworkError::IO)?;
		let bypass_buff_to_read = Arc::new(AtomicUsize::new(0));
		let (sink, mut stream) =
			Framed::new(connection, SataProtoChunker(bypass_buff_to_read.clone())).split();
		let mut first_packet = true;

		let sender = Self::spawn_write_task(disable_load_bearing_sleep, sink)?;

		loop {
			while let Some(result) = stream.next().await {
				let packet = result.map_err(NetworkError::IO)?.freeze();
				let parsed_packet = SataRequest::try_from(packet)?;
				parsed_packet.header().ensure_not_from_host()?;
				if first_packet {
					let flags = SataCapabilitiesFlags(parsed_packet.header().flags());
					if !flags.intersects(SataCapabilitiesFlags::FAST_FILE_IO_SUPPORTED) {
						supports_ffio = false;
					}
					if !flags.intersects(SataCapabilitiesFlags::COMBINED_SEND_RECV_SUPPORTED) {
						supports_csr = false;
					}
				}
				first_packet = false;

				debug!(
					packet = valuable(&parsed_packet),
					"{}",
					parsed_packet.command_info(),
				);
				match parsed_packet.body() {
					SataRequestBody::ChangeMode(ref mode) => {
						sender
							.send(mode.handle(parsed_packet.header(), host_filesystem)?)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::ChangeOwner(ref co) => {
						sender
							.send(co.handle(parsed_packet.header())?)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::CloseFile(ref cf) => {
						sender
							.send(cf.handle(parsed_packet.header(), host_filesystem).await?)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::CloseFolder(ref cf) => {
						sender
							.send(cf.handle(parsed_packet.header(), host_filesystem).await?)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::CreateDirectory(ref cd) => {
						sender
							.send(cd.handle(parsed_packet.header(), host_filesystem).await?)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::GetInfoByQuery(ref info) => {
						sender
							.send(info.handle(parsed_packet.header(), host_filesystem).await?)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::OpenFile(ref file) => {
						sender
							.send(
								file.handle(
									parsed_packet.header(),
									parsed_packet.command_info(),
									host_filesystem,
								)
								.await?,
							)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::OpenFolder(ref folder) => {
						sender
							.send(
								folder
									.handle(parsed_packet.header(), host_filesystem)
									.await?,
							)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::Ping(ref ping) => {
						debug!(
							client.packet.header = valuable(parsed_packet.header()),
							client.packet.command_info = valuable(parsed_packet.command_info()),
							client.packet.body = valuable(ping),
							"received ping packet from client",
						);

						sender
							.send(ping.handle(
								parsed_packet.header(),
								parsed_packet.command_info(),
								supports_ffio,
								supports_csr,
							)?)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::ReadFile(ref rfr) => {
						rfr.handle(
							parsed_packet.header(),
							host_filesystem,
							supports_ffio,
							&sender,
						)
						.await?;
					}
					SataRequestBody::ReadDirectory(ref rd) => {
						sender
							.send(rd.handle(parsed_packet.header(), host_filesystem).await?)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::Remove(ref rm) => {
						sender
							.send(
								rm.handle(
									parsed_packet.header(),
									!disable_real_removal,
									host_filesystem,
								)
								.await?,
							)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::Rewind(ref rewind) => {
						sender
							.send(
								rewind
									.handle(parsed_packet.header(), host_filesystem)
									.await?,
							)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::StatFile(ref st) => {
						sender
							.send(
								SataGetInfoByQueryPacketBody::stat_fd(
									parsed_packet.header(),
									host_filesystem,
									st.file_descriptor(),
								)
								.await?,
							)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
					SataRequestBody::WriteFile(ref wf) => {
						sender
							.send(
								wf.handle(
									parsed_packet.header(),
									host_filesystem,
									supports_ffio,
									&mut stream,
									&bypass_buff_to_read,
								)
								.await?,
							)
							.await
							.map_err(NetworkError::SendQueueFailure)?;
					}
				}
			}
		}
	}
}
