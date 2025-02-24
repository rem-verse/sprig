//! SDIO Protocol implementations live here.
//!
//! NOTE: this is not a traditional SDIO protocol that you may be familiar with
//! this is very specific to the nintendo's CAT-DEV environment. It is also
//! split over two TCP ports.
//!
//! The only part that "has a protocol" too is the control port. The data port
//! is literally just transferring files around.

use crate::{
	errors::NetworkParseError,
	fsemul::sdio::errors::{SDIOAPIError, SDIOProtocolError},
};
use bytes::{BufMut, Bytes, BytesMut};
use tokio::io::Error as IoError;
use tokio_util::codec::{Decoder, Encoder};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// The size of an SDIO Block we end up serving.
pub const SDIO_BLOCK_SIZE: usize = 0x200_usize;
/// The size of an SDIO Block we end up serving.
pub const SDIO_BLOCK_SIZE_AS_U32: u32 = 0x200_u32;
/// The size of a single TCP packet we should end up serving.
pub const SDIO_TCP_PACKET_SIZE: usize = 0x10000_usize;
/// The amount of blocks that can fit within a single packet.
pub const SDIO_BLOCKS_PER_PACKET: usize = SDIO_TCP_PACKET_SIZE / SDIO_BLOCK_SIZE;

/// A codec that chunks a stream into SDIO Control packets.
///
/// All SDIO Control packets are 512 bytes long. ALWAYS.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ChunkSDIOControlCodec;

impl Decoder for ChunkSDIOControlCodec {
	type Item = BytesMut;
	type Error = IoError;

	fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
		if src.len() < 512 {
			Ok(None)
		} else {
			Ok(Some(src.split_to(512)))
		}
	}
}

impl Encoder<Bytes> for ChunkSDIOControlCodec {
	type Error = IoError;

	fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> Result<(), Self::Error> {
		dst.reserve(512);
		dst.extend(item);
		Ok(())
	}
}

/// The types of packets that can be received by the SDIO Control Port.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SdioControlPacketType {
	/// A series of simple log messages, or control type messages.
	Message,
	/// Instruction to read on the data stream.
	Read,
	/// Instruction to write on the data stream.
	Write,
	/// TODO(mythra): confirm this is what it does.
	///
	/// Seems to be used in older firmwares to tell the server to
	/// 'start' the SDIO block channel.
	StartBlockChannel,
	/// TODO(mythra): confirm this is what it does.
	///
	/// Seems to be used in older firmwares to tell the server to
	/// 'start' the CTRL Character channel.
	StartControlListeningChannel,
}

impl From<SdioControlPacketType> for u8 {
	fn from(value: SdioControlPacketType) -> Self {
		match value {
			SdioControlPacketType::Message => 8,
			SdioControlPacketType::Read => 0,
			SdioControlPacketType::Write => 1,
			SdioControlPacketType::StartBlockChannel => 0xA,
			SdioControlPacketType::StartControlListeningChannel => 0xB,
		}
	}
}

impl TryFrom<u8> for SdioControlPacketType {
	type Error = SDIOProtocolError;

	fn try_from(value: u8) -> Result<Self, Self::Error> {
		match value {
			0 => Ok(Self::Read),
			1 => Ok(Self::Write),
			8 => Ok(Self::Message),
			0xA => Ok(Self::StartBlockChannel),
			0xB => Ok(Self::StartControlListeningChannel),
			_ => Err(SDIOProtocolError::UnknownPrintfPacketType(value)),
		}
	}
}

/// Handle a Read Request coming over the SDIO Control port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdioControlReadRequest {
	lba: u32,
	blocks: u32,
	channel: u32,
}

impl SdioControlReadRequest {
	/// Create a new SDIO Read Request to head to the `CONTROL` port.
	///
	/// This assumes you're passing in the LBA as it appears in the DLF file,
	/// (e.g. divisble by [`SDIO_BLOCK_SIZE_AS_U32`]). If you want the raw LBA
	/// you can call [`Self::new_with_raw_lba`].
	///
	/// ## Errors
	///
	/// - If the LBA address is not a possible block address (must be divisble by
	///   [`SDIO_BLOCK_SIZE_AS_U32`]).
	/// - If the channel's first byte is greater than or equal to `0xC` when
	///   encoded to little endian.
	pub fn new(lba: u32, blocks: u32, channel: u32) -> Result<Self, SDIOAPIError> {
		if lba < SDIO_BLOCK_SIZE_AS_U32 || lba % SDIO_BLOCK_SIZE_AS_U32 != 0 {
			return Err(SDIOAPIError::InvalidLBA(lba));
		}
		let as_bytes = channel.to_le_bytes();
		if as_bytes[0] >= 0xC {
			return Err(SDIOAPIError::InvalidChannel(as_bytes[0], channel));
		}

		Ok(Self {
			lba: lba / SDIO_BLOCK_SIZE_AS_U32,
			blocks,
			channel,
		})
	}

	/// Create a new SDIO Read Request to head to the `CONTROL` port.
	///
	/// This assumes you're passing in the LBA as it appears on the network,
	/// (e.g. not the number being divisble by 512).
	///
	/// ## Errors
	///
	/// - If the channel's first byte is greater than or equal to `0xC` when
	///   encoded to little endian.
	pub fn new_with_raw_lba(raw_lba: u32, blocks: u32, channel: u32) -> Result<Self, SDIOAPIError> {
		let as_bytes = channel.to_le_bytes();
		if as_bytes[0] >= 0xC {
			return Err(SDIOAPIError::InvalidChannel(as_bytes[0], channel));
		}

		Ok(Self {
			lba: raw_lba,
			blocks,
			channel,
		})
	}

	/// Get the address to read from.
	#[must_use]
	pub const fn lba(&self) -> u32 {
		self.lba * SDIO_BLOCK_SIZE_AS_U32
	}

	/// Set the "raw" address to read from, this is not in the same form as
	/// `read_request.lba()`.
	///
	/// This is if we took the LBA returned by the LBA block method and divided
	/// it by block size.
	pub fn set_raw_lba(&mut self, new_lba: u32) {
		self.lba = new_lba;
	}

	/// Set the address to read from.
	///
	/// ## Errors
	///
	/// - If the LBA address is not a possible block address (must be divisble by
	///   [`SDIO_BLOCK_SIZE_AS_U32`]).
	pub fn set_lba(&mut self, new_lba: u32) -> Result<(), SDIOAPIError> {
		if new_lba < SDIO_BLOCK_SIZE_AS_U32 || new_lba % SDIO_BLOCK_SIZE_AS_U32 != 0 {
			return Err(SDIOAPIError::InvalidLBA(new_lba));
		}

		self.lba = new_lba / SDIO_BLOCK_SIZE_AS_U32;
		Ok(())
	}

	/// Get the amount of blocks to read.
	#[must_use]
	pub const fn blocks(&self) -> u32 {
		self.blocks
	}

	/// Set the amount of blocks to read from SDIO.
	pub fn set_blocks(&mut self, new_blocks: u32) {
		self.blocks = new_blocks;
	}

	/// Get the channel to read from.
	#[must_use]
	pub const fn channel(&self) -> u32 {
		self.channel
	}

	/// Set the channel this read request is on.
	///
	/// ## Errors
	///
	/// - If the channel's first byte is greater than or equal to `0xC` when
	///   encoded to little endian.
	pub fn set_channel(&mut self, new_channel: u32) -> Result<(), SDIOAPIError> {
		let as_bytes = new_channel.to_le_bytes();
		if as_bytes[0] >= 0xC {
			return Err(SDIOAPIError::InvalidChannel(as_bytes[0], new_channel));
		}

		self.channel = new_channel;
		Ok(())
	}
}

impl TryFrom<Bytes> for SdioControlReadRequest {
	type Error = SDIOProtocolError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if value.len() != 512 {
			return Err(SDIOProtocolError::PrintfInvalidSize(value.len()));
		}
		if value[0] != 0 {
			return Err(SDIOProtocolError::UnknownPrintfPacketType(value[0]));
		}

		let lba = u32::from_le_bytes([value[4], value[5], value[6], value[7]]);
		let blocks = u32::from_le_bytes([value[8], value[9], value[10], value[11]]);
		let channel = u32::from_le_bytes([value[12], value[13], value[14], value[15]]);
		if value[12] >= 0xC {
			return Err(SDIOProtocolError::PrintfInvalidChannel(value[12], channel));
		}

		Ok(Self {
			lba,
			blocks,
			channel,
		})
	}
}

impl From<&SdioControlReadRequest> for Bytes {
	fn from(value: &SdioControlReadRequest) -> Self {
		let mut serialized = BytesMut::with_capacity(512);
		serialized.put_u32_le(0);
		serialized.put_u32_le(value.lba);
		serialized.put_u32_le(value.blocks);
		serialized.put_u32_le(value.channel);
		serialized.extend_from_slice(&[0; 0x1F0]);
		serialized.freeze()
	}
}

impl From<SdioControlReadRequest> for Bytes {
	fn from(value: SdioControlReadRequest) -> Self {
		Self::from(&value)
	}
}

const CONTROL_READ_REQUEST_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("raw_lba"),
	NamedField::new("blocks"),
	NamedField::new("channel"),
];

impl Structable for SdioControlReadRequest {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SdioControlReadRequest",
			Fields::Named(CONTROL_READ_REQUEST_FIELDS),
		)
	}
}

impl Valuable for SdioControlReadRequest {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			CONTROL_READ_REQUEST_FIELDS,
			&[
				Valuable::as_value(&self.lba),
				Valuable::as_value(&self.blocks),
				Valuable::as_value(&self.channel),
			],
		));
	}
}

/// Handle a Write Request coming over the SDIO Control port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdioControlWriteRequest {
	lba: u32,
	blocks: u32,
	channel: u32,
}

impl SdioControlWriteRequest {
	/// Create a new SDIO Write Request to head to the `CONTROL` port.
	///
	/// This assumes you're passing in the LBA as it appears in the DLF file,
	/// (e.g. divisble by [`SDIO_BLOCK_SIZE_AS_U32`]). If you want the raw LBA
	/// you can call [`Self::new_with_raw_lba`].
	///
	/// ## Errors
	///
	/// - If the LBA address is not a possible block address (must be divisble by
	///   [`SDIO_BLOCK_SIZE_AS_U32`]).
	/// - If the channel's first byte is greater than or equal to `0xC` when
	///   encoded to little endian.
	pub fn new(lba: u32, blocks: u32, channel: u32) -> Result<Self, SDIOAPIError> {
		if lba < SDIO_BLOCK_SIZE_AS_U32 || lba % SDIO_BLOCK_SIZE_AS_U32 != 0 {
			return Err(SDIOAPIError::InvalidLBA(lba));
		}
		let as_bytes = channel.to_le_bytes();
		if as_bytes[0] >= 0xC {
			return Err(SDIOAPIError::InvalidChannel(as_bytes[0], channel));
		}

		Ok(Self {
			lba: lba / SDIO_BLOCK_SIZE_AS_U32,
			blocks,
			channel,
		})
	}

	/// Create a new SDIO Write Request to head to the `CONTROL` port.
	///
	/// This assumes you're passing in the LBA as it appears on the network,
	/// (e.g. not the number being divisble by 512).
	///
	/// ## Errors
	///
	/// - If the channel's first byte is greater than or equal to `0xC` when
	///   encoded to little endian.
	pub fn new_with_raw_lba(raw_lba: u32, blocks: u32, channel: u32) -> Result<Self, SDIOAPIError> {
		let as_bytes = channel.to_le_bytes();
		if as_bytes[0] >= 0xC {
			return Err(SDIOAPIError::InvalidChannel(as_bytes[0], channel));
		}

		Ok(Self {
			lba: raw_lba,
			blocks,
			channel,
		})
	}

	/// Get the address to write to.
	#[must_use]
	pub const fn lba(&self) -> u32 {
		self.lba * SDIO_BLOCK_SIZE_AS_U32
	}

	/// Set the "raw" address to write to, this is not in the same form as
	/// `write_request.lba()`.
	///
	/// This is if we took the LBA returned by the LBA block method and divided
	/// it by block size.
	pub fn set_raw_lba(&mut self, new_lba: u32) {
		self.lba = new_lba;
	}

	/// Set the address to write to.
	///
	/// ## Errors
	///
	/// - If the LBA address is not a possible block address (must be divisble by
	///   [`SDIO_BLOCK_SIZE_AS_U32`]).
	pub fn set_lba(&mut self, new_lba: u32) -> Result<(), SDIOAPIError> {
		if new_lba < SDIO_BLOCK_SIZE_AS_U32 || new_lba % SDIO_BLOCK_SIZE_AS_U32 != 0 {
			return Err(SDIOAPIError::InvalidLBA(new_lba));
		}

		self.lba = new_lba / SDIO_BLOCK_SIZE_AS_U32;
		Ok(())
	}

	/// Get the amount of blocks to read.
	#[must_use]
	pub const fn blocks(&self) -> u32 {
		self.blocks
	}

	/// Set the amount of blocks to write to SDIO.
	pub fn set_blocks(&mut self, new_blocks: u32) {
		self.blocks = new_blocks;
	}

	/// Get the channel to read from.
	#[must_use]
	pub const fn channel(&self) -> u32 {
		self.channel
	}

	/// Set the channel this write request is on.
	///
	/// ## Errors
	///
	/// - If the channel's first byte is greater than or equal to `0xC` when
	///   encoded to little endian.
	pub fn set_channel(&mut self, new_channel: u32) -> Result<(), SDIOAPIError> {
		let as_bytes = new_channel.to_le_bytes();
		if as_bytes[0] >= 0xC {
			return Err(SDIOAPIError::InvalidChannel(as_bytes[0], new_channel));
		}

		self.channel = new_channel;
		Ok(())
	}
}

impl TryFrom<Bytes> for SdioControlWriteRequest {
	type Error = SDIOProtocolError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if value.len() != 512 {
			return Err(SDIOProtocolError::PrintfInvalidSize(value.len()));
		}
		if value[0] != 1 {
			return Err(SDIOProtocolError::UnknownPrintfPacketType(value[0]));
		}

		let lba = u32::from_le_bytes([value[4], value[5], value[6], value[7]]);
		let blocks = u32::from_le_bytes([value[8], value[9], value[10], value[11]]);
		let channel = u32::from_le_bytes([value[12], value[13], value[14], value[15]]);
		if value[12] >= 0xC {
			return Err(SDIOProtocolError::PrintfInvalidChannel(value[12], channel));
		}

		Ok(Self {
			lba,
			blocks,
			channel,
		})
	}
}

impl From<&SdioControlWriteRequest> for Bytes {
	fn from(value: &SdioControlWriteRequest) -> Self {
		let mut serialized = BytesMut::with_capacity(512);
		serialized.put_u32_le(1);
		serialized.put_u32_le(value.lba);
		serialized.put_u32_le(value.blocks);
		serialized.put_u32_le(value.channel);
		serialized.extend_from_slice(&[0; 0x1F0]);
		serialized.freeze()
	}
}

impl From<SdioControlWriteRequest> for Bytes {
	fn from(value: SdioControlWriteRequest) -> Self {
		Self::from(&value)
	}
}

const CONTROL_WRITE_REQUEST_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("raw_lba"),
	NamedField::new("blocks"),
	NamedField::new("channel"),
];

impl Structable for SdioControlWriteRequest {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SdioControlReadRequest",
			Fields::Named(CONTROL_WRITE_REQUEST_FIELDS),
		)
	}
}

impl Valuable for SdioControlWriteRequest {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			CONTROL_WRITE_REQUEST_FIELDS,
			&[
				Valuable::as_value(&self.lba),
				Valuable::as_value(&self.blocks),
				Valuable::as_value(&self.channel),
			],
		));
	}
}

#[derive(Clone, Debug, PartialEq, Eq, Valuable)]
pub enum SdioControlMessage {
	/// A message to print to the screen.
	Printf(String),
	/// TODO(mythra): Currently unknown, mostly used for scientist.
	Unknown(Vec<u8>),
}

/// Handle a Write Request coming over the SDIO Control port.
#[derive(Debug, PartialEq, Eq)]
pub struct SdioControlMessageRequest {
	character_length: u16,
	messages: Vec<SdioControlMessage>,
}

impl SdioControlMessageRequest {
	#[must_use]
	pub const fn character_length(&self) -> u16 {
		self.character_length
	}

	#[must_use]
	pub const fn messages(&self) -> &Vec<SdioControlMessage> {
		&self.messages
	}

	#[must_use]
	pub fn messages_owned(self) -> Vec<SdioControlMessage> {
		self.messages
	}
}

impl TryFrom<Bytes> for SdioControlMessageRequest {
	type Error = NetworkParseError;

	#[allow(
    // We will actually loop in the future.
    clippy::never_loop,
  )]
	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if value.len() != 512 {
			return Err(SDIOProtocolError::PrintfInvalidSize(value.len()).into());
		}
		if value[0] != 8 {
			return Err(SDIOProtocolError::UnknownPrintfPacketType(value[0]).into());
		}

		let character_length = u16::from_le_bytes([value[0x2], value[0x3]]);
		let mut messages = Vec::with_capacity(1);
		loop {
			let message_ty = u16::from_le_bytes([
				value[4 + (messages.len() * 4)],
				value[4 + (messages.len() * 4) + 1],
			]);
			// puVar1 & bytes to send after two u16s.
			if message_ty == 4 {
				// first two bytes are unknown
				let mut buff = value.slice((4 + (messages.len() * 4) + 8)..);
				if let Some(eol) = buff.iter().position(|item| *item == 0x0) {
					buff.truncate(eol);
				}
				messages.push(SdioControlMessage::Printf(
					// Yes they sometimes dump junk to us.
					//
					// But alas, what can you do.
					String::from_utf8_lossy(&buff).to_string(),
				));
				// These message types consume the whole buffer probably idk
				break;
			} else if message_ty == 9 {
				messages.push(SdioControlMessage::Unknown(
					value.slice(4 + (messages.len() * 4)..).to_vec(),
				));
				break;
			}

			return Err(SDIOProtocolError::UnknownPrintfMessageType(message_ty).into());
		}

		Ok(Self {
			character_length,
			messages,
		})
	}
}

const MESSAGE_REQUEST_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("character_length"),
	NamedField::new("messages"),
];

impl Structable for SdioControlMessageRequest {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SdioControlMessageRequest",
			Fields::Named(MESSAGE_REQUEST_FIELDS),
		)
	}
}

impl Valuable for SdioControlMessageRequest {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			MESSAGE_REQUEST_FIELDS,
			&[
				Valuable::as_value(&self.character_length),
				Valuable::as_value(&self.messages),
			],
		));
	}
}

#[cfg(feature = "servers")]
pub mod read_packet_temp_will_break {
	//! Functions related to handling reads from the SDIO interface.
	//!
	//! These are the bits that handle all of the translation from an SDIO packet
	//! to a real file being sent back. They mostly are just wrappers around
	//! [`HostFilesystem`] calls, being wrapped in SDIO protocols.

	use crate::{
		errors::{CatBridgeError, FSError, NetworkError},
		fsemul::{
			HostFilesystem,
			dlf::DiskLayoutFile,
			sdio::{
				errors::SDIOProtocolError,
				proto::{SDIO_BLOCK_SIZE, SDIO_BLOCKS_PER_PACKET, SdioControlReadRequest},
			},
		},
	};
	use bytes::{Bytes, BytesMut};
	use std::path::PathBuf;
	use tokio::{
		fs::{File, read as fs_read},
		io::{AsyncReadExt, AsyncSeekExt, BufReader, SeekFrom},
		sync::mpsc::Sender,
	};
	use tracing::{info, warn};

	/// Actually do all the bits to serve a read request, and respond over the
	/// passed in channel.
	///
	/// ## Errors
	///
	/// - If the device requests an unknown address to read files from.
	/// - If we cannot read the file from the disk.
	/// - If we cannot serve the file to a client.
	pub async fn serve_read_request(
		file_system: &HostFilesystem,
		request: &SdioControlReadRequest,
		response_channel: &Sender<Bytes>,
	) -> Result<(), CatBridgeError> {
		let address_to_read = request.lba();
		if address_to_read == 0xFFFF_0000 {
			info!("Requested special ppc_boot.bsf file address");
			let ppc_boot = file_system.boot1_sytstem_path().await?;
			return serve_padded_file_sdio(
				&ppc_boot,
				SeekFrom::Start(0),
				request.blocks(),
				response_channel,
			)
			.await;
		// Probably another special address like diskid or something.
		} else if address_to_read == 0x00F9_0000 {
			info!("Unknown special large address... serving 0 blocks");
			return serve_zeroed_blocks(request.blocks(), response_channel).await;
		}

		let dlf = DiskLayoutFile::try_from(Bytes::from(
			fs_read(file_system.ppc_boot_dlf_path().await?)
				.await
				.map_err(FSError::from)?,
		))?;
		if u128::from(address_to_read) > dlf.max_address() {
			return Err(SDIOProtocolError::AddressOutOfRange(
				u128::from(address_to_read),
				dlf.max_address(),
			)
			.into());
		}

		if let Some((path, offset)) = dlf
			.get_path_and_offset_for_file(u128::from(address_to_read))
			.await
		{
			info!(
				sdio.blocks = request.blocks(),
				sdio.path = %path.display(),
				sdio.path_offset = format!("{:06x}", offset),
				"Serving known file over SDIO",
			);
			serve_padded_file_sdio(
				path,
				SeekFrom::Start(offset),
				request.blocks(),
				response_channel,
			)
			.await
		} else {
			warn!(
				sdio.address = format!("{:02x}", address_to_read),
				sdio.blocks = request.blocks(),
				"Serving unknown address over SDIO",
			);
			serve_zeroed_blocks(request.blocks(), response_channel).await
		}
	}

	/// Serve a file over a tcp stream to SDIO.
	///
	/// Take in a path to a file, the blocks a user is requesting, and serve that
	/// many blocks. If the `blocks_requested` is greater than the contents of
	/// `path`, then we will just serve zero's past the point to fulfill the amount
	/// of `blocks_requested`.
	///
	/// ## Errors
	///
	/// - If we cannot open a file at `path`.
	/// - If you are not running on at least a 32 bit machine, and we cannot turn
	///   `blocks_requested` into a `usize`.
	/// - If we cannot send content over the `response_channel`.
	async fn serve_padded_file_sdio(
		path: &PathBuf,
		offset: SeekFrom,
		blocks_requested: u32,
		response_channel: &Sender<Bytes>,
	) -> Result<(), CatBridgeError> {
		let mut fd = File::open(path).await.map_err(FSError::IO)?;
		fd.seek(offset).await.map_err(FSError::IO)?;
		let mut blocks_left_to_serve = usize::try_from(blocks_requested)
			.map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?;
		// Small enough, ready to just be read one-shot.
		if blocks_left_to_serve <= SDIO_BLOCKS_PER_PACKET {
			let mut file_buff = BytesMut::with_capacity(blocks_left_to_serve * SDIO_BLOCK_SIZE);
			let read_bytes = fd.read_buf(&mut file_buff).await.map_err(FSError::IO)?;
			if read_bytes < blocks_left_to_serve * SDIO_BLOCK_SIZE {
				let padding =
					BytesMut::zeroed((blocks_left_to_serve * SDIO_BLOCK_SIZE) - read_bytes);
				file_buff.extend(padding);
			}

			response_channel
				.send(file_buff.freeze())
				.await
				.map_err(NetworkError::SendQueueFailure)?;
		} else {
			let mut exhausted_file = false;
			let mut reader = BufReader::new(fd);

			while blocks_left_to_serve > 0 {
				let blocks_to_read = std::cmp::min(blocks_left_to_serve, SDIO_BLOCKS_PER_PACKET);
				let bytes_to_read = blocks_to_read * SDIO_BLOCK_SIZE;
				let mut file_buff = BytesMut::with_capacity(bytes_to_read);

				if !exhausted_file {
					let read_bytes = reader.read_buf(&mut file_buff).await.map_err(FSError::IO)?;
					if read_bytes == 0 {
						exhausted_file = true;
					}
				}

				if file_buff.len() < bytes_to_read {
					file_buff.extend(BytesMut::zeroed(bytes_to_read - file_buff.len()));
				}

				response_channel
					.send(file_buff.freeze())
					.await
					.map_err(NetworkError::SendQueueFailure)?;
				blocks_left_to_serve -= blocks_to_read;
			}
		}

		Ok(())
	}

	/// Serve a series of blocks that just contain 0's.
	///
	/// ## Errors
	///
	/// - If you are not running a 32 bit machine, and thus we cannot turn
	///   a u32 into a usize.
	/// - If we cannot send bytes over the `response_channel`.
	async fn serve_zeroed_blocks(
		blocks_requested: u32,
		response_channel: &Sender<Bytes>,
	) -> Result<(), CatBridgeError> {
		let mut blocks_as_size = usize::try_from(blocks_requested)
			.map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?;

		while blocks_as_size > 0 {
			let blocks_to_read = std::cmp::min(blocks_as_size, SDIO_BLOCKS_PER_PACKET);
			let bytes_to_read = blocks_to_read * SDIO_BLOCK_SIZE;

			let zero_buff = BytesMut::zeroed(bytes_to_read);
			response_channel
				.send(zero_buff.freeze())
				.await
				.map_err(NetworkError::SendQueueFailure)?;
			blocks_as_size -= blocks_to_read;
		}

		Ok(())
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[test]
	pub fn decoder_chunks_chunkily() {
		// Empty
		{
			let mut codec = ChunkSDIOControlCodec;
			let mut buff = BytesMut::with_capacity(0);

			codec
				.decode(&mut buff)
				.expect("Failed to call SDIO Control codec on empty.");
		}

		// Too smol owo.
		{
			let mut codec = ChunkSDIOControlCodec;
			let mut buff = BytesMut::zeroed(511);

			assert_eq!(
				codec
					.decode(&mut buff)
					.expect("Failed to call SDIO Control Codec"),
				None,
				"Codec could not successfully decode a buffer that was one byte too short for SDIO Control.",
			);
		}

		// owo are you sure this data can fit???
		{
			let mut codec = ChunkSDIOControlCodec;
			let mut buff = BytesMut::with_capacity(1024);

			buff.extend(vec![0x1; 512]);
			buff.extend(vec![0x2; 512]);

			assert_eq!(
				codec
					.decode(&mut buff)
					.expect("Failed to call SDIO Control Codec"),
				Some(BytesMut::from(&*vec![0x1_u8; 512])),
				"Codec did not successfully decode first part of a too large packet to SDIO Control.",
			);
			assert_eq!(
				codec
					.decode(&mut buff)
					.expect("Failed to call SDIO Control Codec"),
				Some(BytesMut::from(&*vec![0x2_u8; 512])),
				"Codec did not successfully decode second part of a too large packet to SDIO Control."
			);
		}

		// owo okay
		{
			let mut codec = ChunkSDIOControlCodec;
			let mut buff = BytesMut::zeroed(512);

			assert_eq!(
				codec
					.decode(&mut buff)
					.expect("Failed to call SDIO Control Codec"),
				Some(BytesMut::zeroed(512)),
				"Codec did not succsesfully decode a just right packet size to SDIO Control.",
			);
		}
	}

	#[test]
	pub fn roundtrip_control_packet_type() {
		for packet_ty in vec![
			SdioControlPacketType::Message,
			SdioControlPacketType::Read,
			SdioControlPacketType::Write,
		] {
			assert_eq!(
				Ok(packet_ty),
				SdioControlPacketType::try_from(u8::from(packet_ty)),
				"Round-tripped control packet type was not the same?"
			);
		}
	}

	#[test]
	pub fn parse_read_request() {
		// Real life read request packet being parsed.
		{
			let read_request = SdioControlReadRequest::try_from(Bytes::from(vec![
				0x00, 0x00, 0x00, 0x00, 0x80, 0xff, 0x7f, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			]))
			.expect("Failed to parse real life SDIO Control Read Request");

			assert_eq!(
				read_request.lba(),
				0xFFFF_0000,
				"Failed to parse correct address to read from on a real life read request.",
			);
			assert_eq!(
				read_request.blocks(),
				2,
				"Failed to parse correct amount of blocks to read from real life read request.",
			);
			assert_eq!(
				read_request.channel(),
				0,
				"Failed to parse the correct channel to read from on a real life read request.",
			);
		}
	}

	// TODO(mythra): get real life write request.

	#[test]
	pub fn parse_real_life_message_request() {
		{
			let message_request = SdioControlMessageRequest::try_from(Bytes::from(vec![
				0x08, 0x00, 0x3a, 0x00, 0x04, 0x00, 0x0c, 0x00, 0xff, 0xff, 0x3a, 0x00, 0x42, 0x4f,
				0x4f, 0x54, 0x31, 0x3a, 0x20, 0x52, 0x75, 0x6e, 0x6e, 0x69, 0x6e, 0x67, 0x20, 0x44,
				0x55, 0x41, 0x4c, 0x20, 0x62, 0x6f, 0x6f, 0x74, 0x6c, 0x6f, 0x61, 0x64, 0x65, 0x72,
				0x20, 0x61, 0x74, 0x20, 0x30, 0x78, 0x30, 0x38, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30,
				0x2e, 0x0a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x08, 0x40, 0x00, 0x40, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x08, 0x00, 0x00, 0x04,
				0x18, 0x00, 0x00, 0x40, 0x00, 0x00, 0x20, 0x00, 0x02, 0x00, 0x10, 0x00, 0x00, 0x00,
				0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x01, 0x08, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x01, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x10, 0x80, 0x88, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00,
				0x00, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x02, 0x00, 0x40, 0x00, 0x04,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x10, 0x82,
				0x00, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
				0x00, 0x00, 0x40, 0x00, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x04, 0x00, 0x00, 0x20, 0x00, 0x80, 0x00, 0x01, 0x08, 0x00, 0x00, 0x20, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x04, 0x00, 0x00, 0x00, 0x02, 0xc0,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00,
			]))
			.expect("Failed to parse real life SDIO Control Message Request");

			assert_eq!(
				message_request.messages(),
				&vec![SdioControlMessage::Printf(
					"BOOT1: Running DUAL bootloader at 0x08000000.\n".to_owned()
				)],
				"Cannot Parse SDIO Control Message Request.",
			);
		}
	}
}
