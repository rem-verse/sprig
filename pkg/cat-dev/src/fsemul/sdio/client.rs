//! "Client" implementations for SDIO protocols for handling PCFS for cat-dev.
//!
//! Reminder that SDIO Client actually acts like a server, and binds on a port
//! on the machine it's running on. Then the server _connects to us_. This is
//! still exposed at the exact same tcp-client class that you know and
//! hopefully love.

use crate::{
	errors::{CatBridgeError, NetworkError},
	fsemul::sdio::{
		SDIO_DATA_STREAMS,
		data_stream::DataStream,
		errors::SDIONetworkError,
		proto::{
			SDIO_BLOCK_SIZE,
			message::{SdioControlMessage, SdioControlMessageRequest},
			read::SdioControlReadRequest,
		},
	},
	net::client::{TCPClient, models::RequestStreamEvent},
};
use bytes::{Bytes, BytesMut};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::RwLock};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// The default time to wait for successfully sending a packet.
const DEFAULT_SEND_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// A TCP Client capable of interacting with an SDIO server.
#[derive(Debug)]
pub struct SDIOClient {
	/// A TCP listener for accepting new TCP Data stream connections.
	data_listener: Arc<RwLock<TcpListener>>,
	/// How long to wait for a response.
	response_timeout: Duration,
	/// The actual TCP Client we use to stream bytes to and from.
	underlying_client: TCPClient,
}

impl SDIOClient {
	/// Create a new SDIO TCP Client.
	///
	/// This will bind listeners, but will not necissarily ensure a client is
	/// connected. Most internal methods will ensure a client is connected, but
	/// you can
	///
	/// ## Errors
	///
	/// If we cannot bind to any of the ports we need to bind too.
	pub async fn new(
		bind_address_control: SocketAddr,
		bind_address_data: SocketAddr,
		trace_during_debug: bool,
	) -> Result<Self, CatBridgeError> {
		let mut client = TCPClient::new("sdio", 512_usize, (None, None), trace_during_debug);
		let data = Arc::new(RwLock::new(
			TcpListener::bind(bind_address_data)
				.await
				.map_err(NetworkError::IO)?,
		));
		client.bind(bind_address_control).await?;

		let cloned_data_start = data.clone();
		#[cfg(debug_assertions)]
		let copied_trace: bool = trace_during_debug;
		client.set_on_stream_begin(move |event: RequestStreamEvent<()>| async move {
			let sid = event.stream_id();
			let (connection, server_location) = {
				let guard = cloned_data_start.write().await;
				guard.accept().await
			}
			.map_err(NetworkError::IO)?;
			connection.set_nodelay(true).map_err(NetworkError::IO)?;
			let client_location = connection.local_addr().map_err(NetworkError::IO)?;

			_ = SDIO_DATA_STREAMS
				.insert_async(
					sid,
					DataStream::from_stream(
						client_location,
						server_location,
						connection,
						#[cfg(debug_assertions)]
						copied_trace,
					)?,
				)
				.await;

			Ok::<bool, CatBridgeError>(true)
		})?;

		client.set_on_stream_end(|event: RequestStreamEvent<()>| async move {
			SDIO_DATA_STREAMS.remove_async(&event.stream_id()).await;
			Ok(())
		})?;

		Ok(Self {
			data_listener: data,
			response_timeout: DEFAULT_SEND_TIMEOUT,
			underlying_client: client,
		})
	}

	#[must_use]
	pub const fn get_response_timeout(&self) -> Duration {
		self.response_timeout
	}

	/// Log a message over the SDIO printf message.
	///
	/// This message will be sent in 500 byte increments, and the remote side
	/// should always flush on newlines, but certain implementations may differ.
	///
	/// ## Errors
	///
	/// If we cannot send out all of the messages to chunk out on the stream.
	/// NOTE that some _may_ still be sent, as it will only be the first error
	/// that gets bubbled up.
	///
	/// This may mean prior messages may have been sent out first.
	pub async fn log_message(&self, message: String) -> Result<(), CatBridgeError> {
		for char_chunk in message.chars().collect::<Vec<char>>().chunks(500) {
			let msg_chunk = char_chunk.iter().collect::<String>();
			self.underlying_client
				.send(
					SdioControlMessageRequest::new(vec![SdioControlMessage::Printf(
						BytesMut::zeroed(6).freeze(),
						msg_chunk,
					)]),
					None,
				)
				.await?;
		}

		Ok(())
	}

	/// Perform a read request against the remote upstream.
	///
	/// This will read from an address a certain amount of blocks. In real
	/// uses the channel is always the same low number, but you may want to
	/// perform multi channel reads.
	///
	/// ## Errors
	///
	/// If we cannot send a request over the Control stream, or if we cannot read
	/// a response on the data stream.
	pub async fn read(&self, lba: u32, blocks: u32, channel: u32) -> Result<Bytes, CatBridgeError> {
		// Send a message then we'll access the primary streams data stream.
		let (primary_stream_id, _req_id, _) = self
			.underlying_client
			.send(SdioControlReadRequest::new(lba, blocks, channel)?, None)
			.await?;
		let Some(data_stream) = SDIO_DATA_STREAMS.get_async(&primary_stream_id).await else {
			return Err(SDIONetworkError::DataStreamMissing(primary_stream_id).into());
		};

		Ok(data_stream
			.recv(usize::try_from(blocks).unwrap_or(usize::MAX) * SDIO_BLOCK_SIZE)
			.await?)
	}
}

const SDIO_CLIENT_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("data_listener"),
	NamedField::new("underlying_client"),
];

impl Structable for SDIOClient {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static("SDIOClient", Fields::Named(SDIO_CLIENT_FIELDS))
	}
}

impl Valuable for SDIOClient {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SDIO_CLIENT_FIELDS,
			&[
				Valuable::as_value(&format!("{:?}", self.data_listener)),
				Valuable::as_value(&self.underlying_client),
			],
		));
	}
}
