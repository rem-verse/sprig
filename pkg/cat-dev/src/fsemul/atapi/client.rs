//! A simple ATAPI client.
//!
//! This ATAPI client is mostly built to support an ATAPI scientist
//! implementation. That scientist implementation allows communicating with a
//! real implementation of PCFS while serving our own to "diff", and find
//! protocol issues.
//!
//! Hopefully once we understand a lot more about the ATAPI Client we can build
//! a more functional client.

use crate::{errors::NetworkError, fsemul::atapi::proto::ClientChunkedATAPIEmulatorCode};
use bytes::{Bytes, BytesMut};
use futures::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::{
	net::{TcpStream, ToSocketAddrs},
	time::sleep,
};
use tokio_util::codec::Framed;

/// A simple ATAPI client.
#[derive(Debug)]
pub struct ATAPIClient {
	/// The actual underlying connection to an ATAPI server.
	underlying_connection: Framed<TcpStream, ClientChunkedATAPIEmulatorCode>,
}

impl ATAPIClient {
	/// Attmempt to connect to a real ATAPI Server implementation.
	///
	/// ## Errors
	///
	/// If for some reason we cannot open a connection to the ATAPI Server due to
	/// some underlying network error or condition.
	pub async fn connect_to<AddrTy: ToSocketAddrs>(address: AddrTy) -> Result<Self, NetworkError> {
		let conn = TcpStream::connect(address)
			.await
			.map_err(NetworkError::IO)?;
		conn.set_nodelay(true).map_err(NetworkError::IO)?;

		Ok(Self {
			underlying_connection: Framed::new(conn, ClientChunkedATAPIEmulatorCode),
		})
	}

	/// Send a series of raw bytes across the wire.
	///
	/// ## Errors
	///
	/// If we fail to send the packet over the underlying connection.
	pub async fn send_raw(&mut self, buff: Bytes) -> Result<(), NetworkError> {
		self.underlying_connection
			.send(buff)
			.await
			.map_err(NetworkError::IO)
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
	pub async fn try_recv_data(&mut self) -> Result<Option<Bytes>, NetworkError> {
		let mut buff = BytesMut::with_capacity(8192);

		loop {
			tokio::select! {
			  opt_res = self.underlying_connection.next() => {
					let Some(res) = opt_res else {
					  break;
					};
					let new_buff = res.map_err(NetworkError::IO)?;
					buff.extend(new_buff);
			  }
			  () = sleep(Duration::from_millis(100)) => {
					break;
			  }
			}
		}

		if buff.is_empty() {
			Ok(None)
		} else {
			Ok(Some(buff.freeze()))
		}
	}
}
