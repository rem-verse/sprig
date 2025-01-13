//! Servers, and protocols related to ATAPI emulation.
//!
//! ATAPI Emulation is the actual hdd emulation parts of the CAT-DEV.
//! Unlike SDIO, ATAPI acts like EXI and initiates a connection from the CAT-DEV
//! to a host machine.
//!
//! This protocol much like SDIO though, ***IS NOT STANDARD***. I'm sorry if
//! you got a search request for ATAPI, and were looking for actual real ATAPI
//! code. Not this weird nintendo variant.

pub mod proto;
mod reads;

use crate::{
	errors::{APIError, CatBridgeError, NetworkError},
	fsemul::{
		atapi::{proto::ChunkATAPIEmulatorCodec, reads::handle_read_dlf},
		HostFilesystem,
	},
};
use bytes::{BufMut, Bytes, BytesMut};
use futures::{SinkExt, StreamExt};
use local_ip_address::local_ip;
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4};
use tokio::{
	net::{TcpListener, TcpStream},
	task::Builder as TaskBuilder,
};
use tokio_util::codec::Framed;
use tracing::{debug, error, error_span, Instrument};

/// The default port to use for hosting the ATAPI Server.
pub const DEFAULT_ATAPI_PORT: u16 = 7974_u16;

/// An ATAPI Server capable of acting like an HDD for a cat-dev.
#[derive(Debug)]
pub struct AtapiServer<'fs> {
	/// The address we're actively bound and listening on.
	bound_address: SocketAddrV4,
	/// A pointer to our integration with the host filesystem.
	///
	/// This let's us read data from the cafe directory and otherwise.
	host_filesystem: &'fs HostFilesystem,
	/// The listener to serve traffic on.
	server: TcpListener,
}

impl<'fs> AtapiServer<'fs> {
	/// Create and bind a new ATAPI Server to a port on an address.
	///
	/// ## Errors
	///
	/// If we cannot bind to the request host ip address.
	pub async fn new(
		host_filesystem: &'fs HostFilesystem,
		address: Option<Ipv4Addr>,
		port: Option<u16>,
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

		let bound_address = SocketAddrV4::new(ip, port.unwrap_or(DEFAULT_ATAPI_PORT));
		let server = TcpListener::bind(bound_address)
			.await
			.map_err(NetworkError::IO)?;

		Ok(Self {
			bound_address,
			host_filesystem,
			server,
		})
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
	/// [`AtapiServer::serve_concurrently`] to more effeciently serve many
	/// clients at once.
	pub async fn serve(self) {
		let host_filesystem = self.host_filesystem;

		loop {
			match self.server.accept().await {
				Ok((stream, address)) => {
					let client = address;
					let bound = self.bound_address;

					let result = Self::serve_connection(host_filesystem, stream)
						.instrument(error_span!(
						  "cat_dev::fsemul::atapi::serve_connection",
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
				}
				Err(cause) => {
					error!(
					  ?cause,
					  server.address = %self.bound_address,
					  "Failed to accept ATAPI Connection from client, cannot serve itself.",
					);
				}
			}
		}
	}

	/// Serve a connection non-concurrently (E.g. when not dealing with a static
	/// filesystem).
	async fn serve_connection(
		host_filesystem: &'fs HostFilesystem,
		connection: TcpStream,
	) -> Result<(), CatBridgeError> {
		let (mut sink, mut stream) = Framed::new(connection, ChunkATAPIEmulatorCodec).split();

		loop {
			while let Some(result) = stream.next().await {
				let packet = result.map_err(NetworkError::IO)?.freeze();

				match &packet[..2] {
					[0x3, _] => {
						debug!("Would have sent 32 bytes of various descriptions back... not sure which...");
					}
					[0x12, _] => {
						debug!("Would have sent 96 bytes of various descriptions back... not sure which...");
					}
					[0xCF, 0x80] => {
						debug!("ATAPI Event packet sent!");
					}
					[0xF0, _] => {
						debug!("ATAPI 0xF0 called");
						sink.send(Bytes::from(vec![0x0; 4]))
							.await
							.map_err(NetworkError::IO)?;
					}
					[0xF1, 0x00 | 0x02] => {
						debug!("ATAPI 0xF1, 0x0 | 0x02 random data called");
						// I think this is just random data?
						sink.send(Bytes::from(vec![0x69; 32]))
							.await
							.map_err(NetworkError::IO)?;
					}
					[0xF1, 0x01 | 0x03] => {
						debug!("Unknown 0xF1 packet, doesn't do anything on the network...");
					}
					[0xF2, _] => {
						debug!("Got unknown 0xF2 packet: [{packet:02X?}]");
					}
					[0xF3, 0x00] => {
						handle_read_dlf(packet, host_filesystem, &mut sink).await?;
					}
					[0xF3, 0x01] => {
						let mut data = BytesMut::with_capacity(32);
						data.extend_from_slice(b"PC SATA EMUL");
						data.extend_from_slice(&[0_u8; 20]);
						sink.send(data.freeze()).await.map_err(NetworkError::IO)?;
						debug!("Sent `PC SATA EMUL` header!");
					}
					[0xF3, 0x02 | 0x03] | [0xF5 | 0xF7, _] => {
						sink.send(Bytes::from(vec![0x0; 32]))
							.await
							.map_err(NetworkError::IO)?;
						debug!("Sent empty 32 bytes!");
					}
					[0xF6, _] => {
						if packet[1] & 3 != 0 {
							debug!("F6 second byte & 3 != 0, not sending reply!");
						} else {
							let mut data = BytesMut::with_capacity(4);
							data.put_u32_le(1);
							sink.send(data.freeze()).await.map_err(NetworkError::IO)?;
							debug!("Sent F6 reply!");
						}
					}
					_ => {
						debug!("Blackhole-ing: [{packet:02X?}]");
					}
				}
			}
		}
	}
}

impl AtapiServer<'static> {
	/// Actually end up serving connections to clients.
	///
	/// This will continue serving for as long as it is able. This is the
	/// significantly more efficient than just [`AtapiServer::serve`] as we can
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
						.name("cat_dev::fsemul::atapi::serve_client_connection_concurrently")
						.spawn(async move {
							let result =
								Self::serve_connection_concurrently(host_filesystem, stream)
									.instrument(error_span!(
									  "cat_dev::fsemul::atapi::serve_connection_concurrently",
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
						  "Failed to spawn handler for ATAPI Connection, cannot serve task.",
						);
					}
				}
				Err(cause) => {
					error!(
					  ?cause,
					  server.address = %self.bound_address,
					  "Failed to accept ATAPI Connection from client, cannot serve itself.",
					);
				}
			}
		}
	}

	async fn serve_connection_concurrently(
		host_filesystem: &'static HostFilesystem,
		connection: TcpStream,
	) -> Result<(), CatBridgeError> {
		let (mut sink, mut stream) = Framed::new(connection, ChunkATAPIEmulatorCodec).split();

		loop {
			while let Some(result) = stream.next().await {
				let packet = result.map_err(NetworkError::IO)?.freeze();

				match &packet[..2] {
					[0x3, _] => {
						debug!("Would have sent 32 bytes of various descriptions back... not sure which...");
					}
					[0x12, _] => {
						debug!("Would have sent 96 bytes of various descriptions back... not sure which...");
					}
					[0xCF, 0x80] => {
						debug!("ATAPI Event packet sent!");
					}
					[0xF0, _] => {
						sink.send(Bytes::from(vec![0x0; 4]))
							.await
							.map_err(NetworkError::IO)?;
					}
					[0xF1, 0x00 | 0x02] => {
						// I think this is just random data?
						sink.send(Bytes::from(vec![0x69; 32]))
							.await
							.map_err(NetworkError::IO)?;
					}
					[0xF1, 0x01 | 0x03] => {
						debug!("Unknown 0xF1 packet, doesn't do anything on the network...");
					}
					[0xF2, _] => {
						debug!("Got unknown 0xF2 packet: [{packet:02X?}]");
					}
					[0xF3, 0x00] => {
						handle_read_dlf(packet, host_filesystem, &mut sink).await?;
					}
					[0xF3, 0x01] => {
						let mut data = BytesMut::with_capacity(32);
						data.extend_from_slice(b"PC SATA EMUL");
						data.extend_from_slice(&[0_u8; 20]);
						sink.send(data.freeze()).await.map_err(NetworkError::IO)?;
						debug!("Sent `PC SATA EMUL` header!");
					}
					[0xF3, 0x02 | 0x03] | [0xF5 | 0xF7, _] => {
						sink.send(Bytes::from(vec![0x0; 32]))
							.await
							.map_err(NetworkError::IO)?;
						debug!("Sent empty 32 bytes!");
					}
					[0xF6, _] => {
						if packet[1] & 3 != 0 {
							debug!("F6 second byte & 3 != 0, not sending reply!");
						} else {
							let mut data = BytesMut::with_capacity(4);
							data.put_u32_le(1);
							sink.send(data.freeze()).await.map_err(NetworkError::IO)?;
							debug!("Sent F6 reply!");
						}
					}
					_ => {}
				}
			}
		}
	}
}
