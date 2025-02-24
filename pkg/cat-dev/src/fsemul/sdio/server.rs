//! "Server" implementations for SDIO protocols for handling PCFS for cat-dev.
//!
//! These are ironically called "Clients", because the server actually ends up
//! connecting to the client for SDIO. Which is kind of weird, but it's what
//! happens.
//!
//! I promise clients are meant to be in the server feature flag.

use crate::{
	errors::{CatBridgeError, NetworkError},
	fsemul::{
		HostFilesystem,
		sdio::{
			CONNECT_TIMEOUT, DEFAULT_SDIO_BLOCK_PORT, DEFAULT_SDIO_CONTROL_PORT,
			SDIO_TCP_PACKET_BUFFER_SIZE,
			proto::{
				ChunkSDIOControlCodec, SdioControlMessage, SdioControlMessageRequest,
				SdioControlPacketType, SdioControlReadRequest, SdioControlWriteRequest,
				read_packet_temp_will_break::serve_read_request,
			},
		},
	},
};
use bytes::Bytes;
use futures::{SinkExt, StreamExt, stream::SplitSink};
use std::{net::Ipv4Addr, time::Duration};
use tokio::{
	io::AsyncWriteExt,
	net::{TcpStream, tcp::OwnedWriteHalf},
	sync::mpsc::{Sender, channel},
	task::Builder as TaskBuilder,
	time::{sleep, timeout},
};
use tokio_util::codec::Framed;
use tracing::{debug, error, info, trace, warn};

/// Handle all "SDIO" traffic to/from the device.
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
pub struct SdioClient<'fs> {
	control_port: u16,
	control_stream: TcpStream,
	data_port: u16,
	data_stream: TcpStream,
	host_ip: Ipv4Addr,
	host_filesystem: &'fs HostFilesystem,
	no_load_bearing_sleep: bool,
	printf_control_buff: String,
}

impl<'fs> SdioClient<'fs> {
	/// Connect to a MION getting ready to service SDIO requests.
	///
	/// ## Errors
	///
	/// If we cannot connect successfully to the device within the specified
	/// timeout. NOTE: the timeout applies uniquely to each connection. However
	/// we do attempt to connect concurrently so you shouldn't need to wait for
	/// your specified timeout twice.
	pub async fn connect(
		mion_ip: Ipv4Addr,
		control_port: Option<u16>,
		data_port: Option<u16>,
		connect_timeout: Option<Duration>,
		host_filesystem: &'fs HostFilesystem,
		no_load_bearing_sleep: bool,
	) -> Result<Self, CatBridgeError> {
		let control_port = control_port.unwrap_or(DEFAULT_SDIO_CONTROL_PORT);
		let data_port = data_port.unwrap_or(DEFAULT_SDIO_BLOCK_PORT);
		let timeout_duration = connect_timeout.unwrap_or(CONNECT_TIMEOUT);

		// Spawn both timeout tasks so they run concurrently of each other, and
		// user doesn't have to wait for timeout duration * 2.
		let control_stream_future = timeout(
			timeout_duration,
			TcpStream::connect((mion_ip, control_port)),
		);
		let data_stream_future =
			timeout(timeout_duration, TcpStream::connect((mion_ip, data_port)));

		if no_load_bearing_sleep {
			warn!(
				bridge.ip = %mion_ip,
				bridge.control_port = control_port,
				bridge.data_port = data_port,
				"You have disabled our LOAD-BEARING SLEEP for our SDIO connection, if talking to a REAL MION, THIS WILL CAUSE ERRORS.",
			);
		}

		Ok(Self {
			control_port,
			control_stream: control_stream_future
				.await
				.map_err(|_| NetworkError::Timeout(timeout_duration))?
				.map_err(NetworkError::IO)?,
			data_port,
			data_stream: data_stream_future
				.await
				.map_err(|_| NetworkError::Timeout(timeout_duration))?
				.map_err(NetworkError::IO)?,
			host_ip: mion_ip,
			host_filesystem,
			no_load_bearing_sleep,
			printf_control_buff: String::with_capacity(0),
		})
	}

	/// Actually end up serving SDIO Traffic to a CAT-DEV, or other SDIO type
	/// of device.
	///
	/// I know it may be unusual to have a client called "serve", but this is
	/// expected behaviour, although we initiate the connection (because the
	/// OS hasn't started up yet), we do end up acting in a server model.
	///
	/// ## Errors
	///
	/// If we run into any errors actually servicing a device we are actively
	/// connected too.
	pub async fn serve(self) -> Result<(), CatBridgeError> {
		let mut printf_buff = self.printf_control_buff;
		let (sink, mut stream) = Framed::new(self.control_stream, ChunkSDIOControlCodec).split();
		self.data_stream
			.set_nodelay(true)
			.map_err(NetworkError::IO)?;
		let (_data_stream, data_sink) = self.data_stream.into_split();

		let _control_sender = Self::spawn_control_write_task(sink)?;
		let data_sender = Self::spawn_data_write_task(self.no_load_bearing_sleep, data_sink)?;

		while let Some(packet_result) = stream.next().await {
			let packet = match packet_result {
				Ok(packet) => packet.freeze(),
				Err(cause) => return Err(NetworkError::IO(cause).into()),
			};
			// Somehow got an empty packet, not sure what to do with this, let's just close.
			if packet.is_empty() {
				break;
			}

			match SdioControlPacketType::try_from(packet[0])? {
				SdioControlPacketType::Message => {
					let message_request =
						SdioControlMessageRequest::try_from(packet).map_err(NetworkError::Parse)?;
					for message in message_request.messages_owned() {
						match message {
							SdioControlMessage::Printf(to_print) => {
								printf_buff.push_str(&to_print);
							}
							SdioControlMessage::Unknown(ref buff) => {
								debug!(
									buff = format!("{:02X?}", buff),
									"Unknown message type == 9 for SDIO, Not Sure How to Respond?"
								);
							}
						}
					}

					printf_buff = Self::process_log_messages(
						printf_buff,
						self.host_ip,
						self.control_port,
						self.data_port,
					);
				}
				SdioControlPacketType::Read => {
					let read_request = SdioControlReadRequest::try_from(packet)?;
					serve_read_request(self.host_filesystem, &read_request, &data_sender).await?;
				}
				SdioControlPacketType::Write => {
					let write_request = SdioControlWriteRequest::try_from(packet)?;
					todo!("Unsure how to handle SDIO Write Request: {write_request:?}");
				}
				SdioControlPacketType::StartBlockChannel => {
					info!(
						"Got request to start PCFS Block Channel, but we've already started it..."
					);
				}
				SdioControlPacketType::StartControlListeningChannel => {
					info!(
						"Got request to start CTRL Character Channel, but we've already started it..."
					);
				}
			}
		}

		Ok(())
	}

	fn process_log_messages(
		mut printf_buff: String,
		host_ip: Ipv4Addr,
		control_port: u16,
		data_port: u16,
	) -> String {
		let mut used_one = false;

		loop {
			while let Some(line_ending) = printf_buff.find('\n') {
				used_one = true;
				let remaining = printf_buff.split_off(line_ending + 1);
				let actual_line: String = printf_buff;
				printf_buff = remaining;

				// Ignore empty newlines they try to send.
				if !actual_line.trim().is_empty() {
					info!(
						sdio.host_ip = %host_ip,
						sdio.host_control_port = control_port,
						sdio.host_data_port = data_port,
						sdio.data.printf = %actual_line.trim(),
						"Received SDIO message.",
					);
				}
			}
			while let Some(line_ending) = printf_buff.find('\r') {
				used_one = true;
				let remaining = printf_buff.split_off(line_ending + 1);
				let actual_line: String = printf_buff;
				printf_buff = remaining;

				// Ignore empty newlines they try to send.
				if !actual_line.trim().is_empty() {
					info!(
						sdio.host_ip = %host_ip,
						sdio.host_control_port = control_port,
						sdio.host_data_port = data_port,
						sdio.data.printf = %actual_line.trim(),
						"Received SDIO message.",
					);
				}
			}

			if !used_one {
				break;
			}
			used_one = false;
		}

		printf_buff
	}

	/// Spawn a task that will watch a channel, and send it out over a channel.
	fn spawn_control_write_task(
		mut sink: SplitSink<Framed<TcpStream, ChunkSDIOControlCodec>, Bytes>,
	) -> Result<Sender<Bytes>, CatBridgeError> {
		let (sender, mut receiver) = channel::<Bytes>(SDIO_TCP_PACKET_BUFFER_SIZE);

		TaskBuilder::new()
			.name("cat_dev::fsemul::sdio_control::write_task")
			.spawn(async move {
				while let Some(packet) = receiver.recv().await {
					if let Err(cause) = sink.send(packet).await {
						error!(
							?cause,
							"Failed to send packet over SDIO Control, error in write channel, shutting down",
						);
						break;
					}
				}
			})
			.map_err(CatBridgeError::SpawnFailure)?;

		Ok(sender)
	}

	/// Spawn a task that will watch a channel, and send it out over a channel.
	fn spawn_data_write_task(
		disable_load_bearing_sleep: bool,
		mut sink: OwnedWriteHalf,
	) -> Result<Sender<Bytes>, CatBridgeError> {
		let (sender, mut receiver) = channel::<Bytes>(SDIO_TCP_PACKET_BUFFER_SIZE);

		TaskBuilder::new()
			.name("cat_dev::fsemul::sdio_data::write_task")
			.spawn(async move {
				while let Some(packet) = receiver.recv().await {
					if let Err(cause) = sink.write(&packet).await {
						error!(
							?cause,
							"Failed to send packet over SDIO Data, error in write channel, shutting down",
						);
						break;
					}

					if !disable_load_bearing_sleep {
						// Yes, this is a very load bearing sleep.
						//
						// Without this sleep, when sending large amounts of data such as the
						// initial `fw.img` packet which is 15360 blocks (or 7,864,320
						// bytes), you'll send all the bytes, the cat-dev will ack them all,
						// and you'll see the following line in debug logs:
						//
						// `24, TS - 8226, Msg - --- DBG: [SDIO]  >CH1 Read 6717440 / 7864320 byte`
						//
						// Yes that's right, it only read about 6 million of the bytes, not all 7
						// million. EVEN THOUGH IT TCP ACKED ALL 7 MILLION.
						//
						// A user can techincally turn this off, BUT you will notice spurious
						// errors.
						trace!("sleeping to work around MION TCP buffer-bug...");
						sleep(Duration::from_millis(25)).await;
					}
				}
			})
			.map_err(CatBridgeError::SpawnFailure)?;

		Ok(sender)
	}
}

impl SdioClient<'static> {
	/// Actually end up serving SDIO Traffic to a CAT-DEV, or other SDIO type
	/// of device.
	///
	/// I know it may be unusual to have a client called "serve", but this is
	/// expected behaviour, although we initiate the connection (because the
	/// OS hasn't started up yet), we do end up acting in a server model.
	///
	/// This server is capable of receiving multiple requests concurrently.
	/// Sending over the data port is still single-threaded, because the
	/// protocol inherently doesn't allow for it, but you can at least
	/// service control requests while waiting on the data-stream with this
	/// server.
	///
	/// ## Errors
	///
	/// If we run into any errors actually servicing a device we are actively
	/// connected too.
	pub async fn serve_concurrently(self) -> Result<(), CatBridgeError> {
		let mut printf_buff = self.printf_control_buff;
		let (sink, mut stream) = Framed::new(self.control_stream, ChunkSDIOControlCodec).split();
		let (_data_stream, data_sink) = self.data_stream.into_split();

		let _control_sender = Self::spawn_control_write_task(sink)?;
		let data_sender = Self::spawn_data_write_task(self.no_load_bearing_sleep, data_sink)?;

		while let Some(packet_result) = stream.next().await {
			let packet = match packet_result {
				Ok(packet) => packet.freeze(),
				Err(cause) => return Err(NetworkError::IO(cause).into()),
			};
			// Somehow got an empty packet, not sure what to do with this, let's just close.
			if packet.is_empty() {
				break;
			}

			match SdioControlPacketType::try_from(packet[0])? {
				SdioControlPacketType::Message => {
					let message_request = SdioControlMessageRequest::try_from(packet)?;
					for message in message_request.messages_owned() {
						match message {
							SdioControlMessage::Printf(to_print) => {
								printf_buff.push_str(&to_print);
							}
							SdioControlMessage::Unknown(ref buff) => {
								debug!(
									buff = format!("{:02X?}", buff),
									"Unknown message type == 9 for SDIO, Not Sure How to Respond?"
								);
							}
						}
					}

					printf_buff = Self::process_log_messages(
						printf_buff,
						self.host_ip,
						self.control_port,
						self.data_port,
					);
				}
				SdioControlPacketType::Read => {
					let read_request = SdioControlReadRequest::try_from(packet)?;

					let host_fs: &'static HostFilesystem = self.host_filesystem;
					let cloned_sender = data_sender.clone();
					TaskBuilder::new()
						.name("cat_dev::fsemul::sdio::serve_read_concurrently")
						.spawn(async move {
							if let Err(cause) =
								serve_read_request(host_fs, &read_request, &cloned_sender).await
							{
								error!(?cause, "Failed to respond to read request, ignoring!");
							}
						})
						.map_err(CatBridgeError::SpawnFailure)?;
				}
				SdioControlPacketType::Write => {
					let _write_request = SdioControlWriteRequest::try_from(packet)?;
				}
				SdioControlPacketType::StartBlockChannel => {
					info!(
						"Got request to start PCFS Block Channel, but we've already started it..."
					);
				}
				SdioControlPacketType::StartControlListeningChannel => {
					info!(
						"Got request to start CTRL Character Channel, but we've already started it..."
					);
				}
			}
		}

		Ok(())
	}
}
