//! Servers, and protocols related to ATAPI emulation.
//!
//! ATAPI Emulation is the actual disk drive emulation parts of the CAT-DEV.
//! Unlike SDIO ATAPI acts like EXI and initiates a connection from the CAT-DEV
//! to a host machine.
//!
//! This protocol much like SDIO though, ***IS NOT STANDARD***. I'm sorry if
//! you got a search request for ATAPI, and were looking for actual real ATAPI
//! code. Not this weird nintendo variant.

pub mod proto;

use crate::{
	errors::{APIError, CatBridgeError, NetworkError},
	fsemul::atapi::proto::ChunkATAPIEmulatorCodec,
};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use local_ip_address::local_ip;
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4};
use tokio::{
	net::{TcpListener, TcpStream},
	task::Builder as TaskBuilder,
};
use tokio_util::codec::Framed;
use tracing::{error, error_span, info, Instrument};

/// The default port to use for hosting the ATAPI Emulator.
pub const DEFAULT_ATAPI_PORT: u16 = 7974_u16;

#[derive(Debug)]
pub struct AtapiServer {
	/// The address we're actively bound and listening on.
	bound_address: SocketAddrV4,
	/// The listener to serve traffic on.
	server: TcpListener,
}

impl AtapiServer {
	/// Create and bind a new ATAPI Server to a port on an address.
	///
	/// ## Errors
	///
	/// If we cannot bind to the request host ip address.
	pub async fn new(
		address: Option<Ipv4Addr>,
		port: Option<u16>,
	) -> Result<AtapiServer, CatBridgeError> {
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
			.map_err(NetworkError::IOError)?;

		Ok(Self {
			bound_address,
			server,
		})
	}

	/// Actually end up serving connections to clients.
	///
	/// This will continue serving for as long as it is able..
	pub async fn serve(self) {
		loop {
			match self.server.accept().await {
				Ok((stream, address)) => {
					let client = address;
					let bound = self.bound_address;
					let spawn_result = TaskBuilder::new()
						.name("cat_dev::fsemul::atapi::serve_client_connection")
						.spawn(async move {
							let result = Self::serve_connection(stream)
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

	async fn serve_connection(connection: TcpStream) -> Result<(), CatBridgeError> {
		let (mut sink, mut stream) = Framed::new(connection, ChunkATAPIEmulatorCodec).split();

		loop {
			while let Some(result) = stream.next().await {
				let packet = result.map_err(NetworkError::IOError)?.freeze();

				if packet.starts_with(&[0xCF, 0x80]) {
					// TODO(mythra): this seems to be some sort of either unlock disk, or diagnostics.
					//               get sent during startup with no responses.
					info!("Received 0xCF 0x80 message, ignoring...");
				} else if packet.starts_with(&[0xF2, 0x07]) {
					// I think this is "unlock" drive or something similar.
					info!("Received 0xF2 0x07 message, ignoring...");
				} else if packet.starts_with(&[0xF3, 0x01]) {
					info!("Sending probably drive ready message...");
					sink.send(Bytes::from(vec![
						0x50, 0x43, 0x20, 0x53, 0x41, 0x54, 0x41, 0x20, 0x45, 0x4d, 0x55, 0x4C,
						0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
						0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
					]))
					.await
					.map_err(NetworkError::IOError)?;
				} else if packet.starts_with(&[0xF3, 0x02]) {
					info!("Sending probably ReadDiscSerialID");
					sink.send(Bytes::from(vec![0; 32]))
						.await
						.map_err(NetworkError::IOError)?;
				} else if packet.starts_with(&[0xF6, 0x02]) {
					info!("Saw 0xF6 0x02, not sure what to do, ignoring....");
				} else if packet.starts_with(&[0xF3, 0x00]) {
					info!("Saw 'malformed' packet, doing stuff.");
					sink.send(Bytes::from(vec![0; 4380]))
						.await
						.map_err(NetworkError::IOError)?;
					sink.send(Bytes::from(vec![0; 5840]))
						.await
						.map_err(NetworkError::IOError)?;
					sink.send(Bytes::from(vec![0; 11680]))
						.await
						.map_err(NetworkError::IOError)?;
					sink.send(Bytes::from(vec![0; 26280]))
						.await
						.map_err(NetworkError::IOError)?;
					sink.send(Bytes::from(vec![0; 49640]))
						.await
						.map_err(NetworkError::IOError)?;
					sink.send(Bytes::from(vec![0; 64240]))
						.await
						.map_err(NetworkError::IOError)?;
					sink.send(Bytes::from(vec![0; 62780]))
						.await
						.map_err(NetworkError::IOError)?;
					sink.send(Bytes::from(vec![0; 37304]))
						.await
						.map_err(NetworkError::IOError)?;
				}
			}
		}
	}
}
