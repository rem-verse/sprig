//! Utility to 'expose' the "raw data stream".
//!
//! In reality this is just a thin wrapper around spawning a task that reads,
//! and writes, and shuts itself down when the stream disconnects.

use crate::{
	errors::{CatBridgeError, NetworkError},
	fsemul::sdio::errors::SDIONetworkError,
};
use bytes::{Bytes, BytesMut};
use std::{
	net::{Ipv4Addr, SocketAddr, SocketAddrV4},
	time::Duration,
};
use tokio::{
	io::{AsyncReadExt, AsyncWriteExt},
	net::TcpStream,
	sync::{
		Mutex,
		mpsc::{Receiver as BoundedReceiver, Sender as BoundedSender, channel as bounded_channel},
	},
	task::Builder as TaskBuilder,
	time::sleep,
};
use tracing::{Instrument, error, error_span};

#[cfg(debug_assertions)]
use tracing::debug;

/// A connection to a "data stream" for SDIO.
///
/// This structure really does two things:
///
/// 1. Provide a way to interact with the data-stream task that exposes
///    arbitrary `write`/`read` calls.
/// 2. Holds onto a 'sender' that when this structure is dropped, will drop
///    the channel, which causes the data stream task to also shut its self
///    down.
#[derive(Debug)]
pub struct DataStream {
	/// A small wrapper around the read interface to read bytes from the string.
	///
	/// We do this in a lock because reading is actually split across two
	/// channels. We send one request to request a read of N bytes, and then we
	/// receive from another channel the data we read.
	read_bytes: Mutex<DataStreamReadInterface>,
	/// An interface to send bytes out of the data-stream.
	send_bytes: BoundedSender<Bytes>,
}

impl DataStream {
	pub fn from_stream(
		client_address: SocketAddr,
		server_address: SocketAddr,
		stream: TcpStream,
		#[cfg(debug_assertions)] trace_io: bool,
	) -> Result<Self, CatBridgeError> {
		let (request_read_sender, request_read_receiver) = bounded_channel(128);
		let (read_response_sender, read_response_receiver) = bounded_channel(128);
		let (send_bytes_sender, send_bytes_receiver) = bounded_channel(128);

		TaskBuilder::new()
			.name("cat_dev::fsemul::sdio::client::data_stream_handler")
			.spawn(async move {
				do_data_stream(
					None,
					None,
					stream,
					request_read_receiver,
					read_response_sender,
					send_bytes_receiver,
					#[cfg(debug_assertions)]
					trace_io,
				)
				.instrument(error_span!(
				  "FSEmulSDIOClientDataStream",
				  client.address = %client_address,
				  server.address = %server_address,
				  client.service = "sdio.data",
				))
				.await;
			})
			.map_err(CatBridgeError::SpawnFailure)?;

		Ok(Self {
			read_bytes: Mutex::new(DataStreamReadInterface::new(
				request_read_sender,
				read_response_receiver,
			)),
			send_bytes: send_bytes_sender,
		})
	}

	/// Create, and connect to the SDIO Data Stream.
	///
	/// This gets called when an SDIO Control stream starts.
	///
	/// ## Errors
	///
	/// If we cannot connect to the remote data stream, or spawn a background
	/// task to process that connection.
	pub async fn connect(
		address: SocketAddr,
		cat_dev_sleep_for: Option<Duration>,
		chunk_at_size: Option<usize>,
		#[cfg(debug_assertions)] trace_io: bool,
	) -> Result<Self, CatBridgeError> {
		let stream = TcpStream::connect(address)
			.await
			.map_err(NetworkError::IO)?;
		stream.set_nodelay(true).map_err(NetworkError::IO)?;

		let (request_read_sender, request_read_receiver) = bounded_channel(128);
		let (read_response_sender, read_response_receiver) = bounded_channel(128);
		let (send_bytes_sender, send_bytes_receiver) = bounded_channel(128);

		TaskBuilder::new()
			.name("cat_dev::fsemul::sdio::server::data_stream_handler")
			.spawn(async move {
				let client_address = address;
				let server_address = stream
					.local_addr()
					.unwrap_or_else(|_| SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)));

				do_data_stream(
					cat_dev_sleep_for,
					chunk_at_size,
					stream,
					request_read_receiver,
					read_response_sender,
					send_bytes_receiver,
					#[cfg(debug_assertions)]
					trace_io,
				)
				.instrument(error_span!(
				  "FSEmulSDIOServerDataStream",
				  client.address = %client_address,
				  server.address = %server_address,
				  client.service = "sdio.data",
				))
				.await;
			})
			.map_err(CatBridgeError::SpawnFailure)?;

		Ok(Self {
			read_bytes: Mutex::new(DataStreamReadInterface::new(
				request_read_sender,
				read_response_receiver,
			)),
			send_bytes: send_bytes_sender,
		})
	}

	/// Send bytes to the data stream.
	///
	/// ## Errors
	///
	/// If we cannot queue any more packets to be sent out.
	pub async fn send(&self, bytes: Bytes) -> Result<(), SDIONetworkError> {
		Ok(self.send_bytes.send(bytes).await?)
	}

	/// Receive N bytes from the data stream.
	///
	/// ## Errors
	///
	/// If we cannot read those bytes from the stream because it is closed, or
	/// ran into some other error.
	pub async fn recv(&self, bytes: usize) -> Result<Bytes, SDIONetworkError> {
		let mut guard = self.read_bytes.lock().await;
		guard.read(bytes).await
	}
}

/// The core loop of an SDIO data stream, just a thin wrapper around bounded
/// chanenels that expose write/recv.
async fn do_data_stream(
	cat_dev_sleep_for: Option<Duration>,
	chunk_at_size: Option<usize>,
	mut raw_stream: TcpStream,
	mut request_read_receiver: BoundedReceiver<usize>,
	read_response_sender: BoundedSender<Bytes>,
	mut send_bytes_receiver: BoundedReceiver<Bytes>,
	#[cfg(debug_assertions)] trace_io: bool,
) {
	loop {
		tokio::select! {
		  opt = request_read_receiver.recv() => {
			  let Some(bytes_to_read) = opt else {
				  break;
			  };

			  let mut buff = BytesMut::zeroed(bytes_to_read);
			  if let Err(cause) = raw_stream.read_exact(&mut buff).await {
				  error!(?cause, requested_bytes = bytes_to_read, "Could not read bytes from data stream");
				  break;
			  }

				#[cfg(debug_assertions)]
				if trace_io {
					debug!(
						body.hex = format!("{buff:02x?}"),
						body.str = String::from_utf8_lossy(&buff).to_string(),
						"cat-dev-trace-input-data-stream",
					);
				}

			  if let Err(cause) = read_response_sender.send(buff.freeze()).await {
				  error!(?cause, "Could not send response back out that we received from data stream");
				  break;
			  }
		  }
		  opt = send_bytes_receiver.recv() => {
			  let Some(bytes_to_send) = opt else {
				  break;
			  };

				let messages = if let Some(chunk_size) = chunk_at_size {
					bytes_to_send.chunks(chunk_size)
						.map(Bytes::copy_from_slice)
						.collect::<Vec<_>>()
				} else {
					vec![bytes_to_send]
				};

				for message in messages {
					#[cfg(debug_assertions)]
					if trace_io {
						debug!(
							body.hex = format!("{message:02x?}"),
							body.str = String::from_utf8_lossy(&message).to_string(),
							"cat-dev-trace-output-data-stream",
						);
					}

					if let Err(cause) = raw_stream.writable().await {
					  error!(?cause, "Could not wait for data stream to be writable");
						break;
					}
				  if let Err(cause) = raw_stream.write_all(&message).await {
					  error!(?cause, "Could not write response to data stream");
					  break;
				  }
				  if let Some(sleep_for) = cat_dev_sleep_for {
					  sleep(sleep_for).await;
				  }
				}
		  }
		}
	}
}

/// See the comments on [`DataStream::read_bytes`] for more info about why the
/// read interface is separated out.
#[derive(Debug)]
struct DataStreamReadInterface {
	/// Send a request to tell the data stream to read a certain amount of bytes.
	request_data_stream_to_read: BoundedSender<usize>,
	/// The read bytes from the last request.
	read_bytes: BoundedReceiver<Bytes>,
}

impl DataStreamReadInterface {
	/// Create a new reader interface that allows reading from a data stream.
	#[must_use]
	pub const fn new(
		request_send: BoundedSender<usize>,
		request_recv: BoundedReceiver<Bytes>,
	) -> Self {
		Self {
			request_data_stream_to_read: request_send,
			read_bytes: request_recv,
		}
	}

	/// Read N bytes from the stream.
	///
	/// ## Errors
	///
	/// If we cannot send, or receive bytes from this data stream interface.
	pub async fn read(&mut self, bytes_to_read: usize) -> Result<Bytes, SDIONetworkError> {
		// Request a read of N bytes...
		self.request_data_stream_to_read.send(bytes_to_read).await?;
		// Now get it's response.
		self.read_bytes
			.recv()
			.await
			.ok_or_else(|| SDIONetworkError::DataStreamDidNotRespond)
	}
}
