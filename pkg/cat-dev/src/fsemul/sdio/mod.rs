//! Clients, and Protocols for interacting with the CAT-DEV's SDIO.
//!
//! Unlike traditional SDIO which is usually low-level firmware level protocol
//! that involves things like individual bits for the protocol, the CAT-DEV
//! takes a very 'interesting' approach to what they call SDIO.
//!
//! Specifically the first thing to note is that, "SDIO" can refer to two
//! ports on your CAT-DEV. There is "SDIO Printf/Control" (by default port
//! 7975), and "SDIO Block Data" (by default port 7976), which actually
//! interact over two totally independent TCP streams.

pub mod errors;
pub mod proto;
mod reads;

use crate::{
	errors::{CatBridgeError, NetworkError},
	fsemul::{
		sdio::{
			proto::{
				ChunkSDIOControlCodec, SdioControlMessage, SdioControlMessageRequest,
				SdioControlPacketType, SdioControlReadRequest, SdioControlWriteRequest,
			},
			reads::serve_read_request,
		},
		HostFilesystem,
	},
};
use bytes::Bytes;
use futures::{stream::SplitSink, SinkExt, StreamExt};
use std::{net::Ipv4Addr, time::Duration};
use tokio::{
	io::AsyncWriteExt,
	net::{tcp::OwnedWriteHalf, TcpStream},
	sync::{
		mpsc::{channel, Sender},
		Mutex,
	},
	task::Builder as TaskBuilder,
	time::timeout,
};
use tokio_util::codec::Framed;
use tracing::{error, info};

/// The default port to use for "SDIO Printf/Control" communications.
///
/// It should be noted that a human can override this port in the MION itself.
/// However, most nintendo tools don't get this param from the MION itself, and
/// expect it to also be set in `fsemul.ini`.
pub const DEFAULT_SDIO_CONTROL_PORT: u16 = 7975;
/// The default port to use for "SDIO Block Data" communications.
///
/// It should be noted that a human can override this port in the MION itself.
/// However, most nintendo tools don't get this param from the MION itself, and
/// expect it to also be set in `fsemul.ini`.
pub const DEFAULT_SDIO_BLOCK_PORT: u16 = 7976;

/// The timeout to initiate a TCP connection to SDIO.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// The amount of TCP Packets that can be buffered per client.
///
/// *note: this is not the size of an individual packet, or packets, but is
/// just the amount of packets that can be queued.*
const SDIO_TCP_PACKET_BUFFER_SIZE: usize = 8192_usize;

/// A lock to ensure only one person sends over SDIO at a time.
///
/// This is required because SDIO Data just transfers bytes with
/// no end identifiers, and no identifiers to which file/channel
/// /etc. it's sending for.
static SDIO_DATA_LOCK: Mutex<()> = Mutex::const_new(());

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
		let (_data_stream, data_sink) = self.data_stream.into_split();

		let _control_sender = Self::spawn_control_write_task(sink)?;
		let data_sender = Self::spawn_data_write_task(data_sink)?;

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
					let guard = SDIO_DATA_LOCK.lock().await;
					serve_read_request(self.host_filesystem, &read_request, &data_sender).await?;
					std::mem::drop(guard);
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
					info!("Got request to start CTRL Character Channel, but we've already started it...");
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

				info!(
					sdio.host_ip = %host_ip,
					sdio.host_control_port = control_port,
					sdio.host_data_port = data_port,
					sdio.data.printf = %actual_line.trim(),
					"Received SDIO message.",
				);
			}
			while let Some(line_ending) = printf_buff.find('\r') {
				used_one = true;
				let remaining = printf_buff.split_off(line_ending + 1);
				let actual_line: String = printf_buff;
				printf_buff = remaining;

				info!(
					sdio.host_ip = %host_ip,
					sdio.host_control_port = control_port,
					sdio.host_data_port = data_port,
					sdio.data.printf = %actual_line.trim(),
					"Received SDIO message.",
				);
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
	fn spawn_data_write_task(mut sink: OwnedWriteHalf) -> Result<Sender<Bytes>, CatBridgeError> {
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
		let data_sender = Self::spawn_data_write_task(data_sink)?;

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
							let guard = SDIO_DATA_LOCK.lock().await;
							if let Err(cause) =
								serve_read_request(host_fs, &read_request, &cloned_sender).await
							{
								error!(?cause, "Failed to respond to read request, ignoring!");
							}
							std::mem::drop(guard);
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
					info!("Got request to start CTRL Character Channel, but we've already started it...");
				}
			}
		}

		Ok(())
	}
}
