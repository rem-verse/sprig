//! Protocols for the SATA 'PCFS' Protocol.
//!
//! This top level function is just all of the things that are common across
//! the entire protocol, each packet serializer/deserializer/handler is kept
//! in it's own sub-file.

mod ping;

use crate::{
	errors::NetworkParseError,
	fsemul::pcfs::errors::{PCFSApiError, PCFSSataProtocolError},
};
use bytes::{Bytes, BytesMut};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::Error as IoError;
use tokio_util::codec::{Decoder, Encoder};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

pub use crate::fsemul::pcfs::sata_proto::ping::*;

/// A codec that chunks a stream into full PCFS packets.
///
/// PCFS will have a data header of `0x20` bytes, and then a data length
/// defined as the first four bytes in a packet.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PCFSSataProtoChunker;

impl Decoder for PCFSSataProtoChunker {
	type Item = BytesMut;
	type Error = IoError;

	fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
		// We don't yet have a complete header...
		if src.len() < 0x20 {
			return Ok(None);
		}

		let data_len = u32::from_be_bytes([src[0], src[1], src[2], src[3]]);
		let final_len = usize::try_from(0x20 + data_len).unwrap_or(usize::MAX);

		if src.len() < final_len {
			Ok(None)
		} else {
			Ok(Some(src.split_to(final_len)))
		}
	}
}

impl Encoder<Bytes> for PCFSSataProtoChunker {
	type Error = IoError;

	fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> Result<(), Self::Error> {
		if item.len() < 0x20 {
			return Err(IoError::other(PCFSApiError::PacketMissingHeader(
				item.len(),
			)));
		}
		let data_len = u32::from_be_bytes([item[0], item[1], item[2], item[3]]);
		let final_len = usize::try_from(0x20 + data_len).unwrap_or(usize::MAX);

		if item.len() != final_len {
			return Err(IoError::other(PCFSApiError::IncorrectSizePacket(
				final_len,
				item.len(),
			)));
		}

		dst.reserve(item.len());
		dst.extend(item);
		Ok(())
	}
}

/// A series of bitflags that are on the packet header field.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PCFSSataPacketHeaderFields(pub u32);

bitflags::bitflags! {
	impl PCFSSataPacketHeaderFields: u32 {
		const FAST_FILE_IO_SUPPORTED = 0x2;
		const COMBINED_SEND_RECV_SUPPORTED = 0x4;
	}
}

/// The header for all of our PCFS packets.
#[derive(Debug, PartialEq, Eq)]
pub struct PCFSSataPacketHeader {
	/// The length of the packet after the header.
	packet_data_len: u32,
	/// An "ID" for packets, THIS IS NOT THE PACKET TYPE.
	///
	/// This seemingly more used to correlate requests/responses on the client
	/// side incase it sends multiple packets at once.
	packet_id: u32,
	/// A bitfield of various flags in the header.
	flags: PCFSSataPacketHeaderFields,
	/// The version of the PCFS protocol to use.
	///
	/// *note: this will be 0 at start, and then be filled in by the first
	/// server response.*
	version: u32,
	/// The timestamp of the host. Yes, they are not ready for the 32 bit
	/// overflow.
	///
	/// For us we actually end up *wrapping* around on our timestamp.
	timestamp_on_host: u32,
	/// The pid opf the running host process.
	pid_on_host: u32,
	// There are techincally 8 bytes of 0x0 that are used for 'padding'
}

impl PCFSSataPacketHeader {
	#[must_use]
	pub const fn data_len(&self) -> u32 {
		self.packet_data_len
	}

	#[must_use]
	pub const fn id(&self) -> u32 {
		self.packet_id
	}

	#[must_use]
	pub const fn flags(&self) -> PCFSSataPacketHeaderFields {
		self.flags
	}

	#[must_use]
	pub const fn version(&self) -> u32 {
		self.version
	}

	#[must_use]
	pub const fn raw_timestamp_on_host(&self) -> u32 {
		self.timestamp_on_host
	}

	/// A "timestamp on the host".
	///
	/// Because this timestamp is *NOT* 32 bit epoch safe. These timestamps
	/// may be *VERY WRONG* on purpose if it's past the year 2038. Don't come
	/// blame me. Blame nintendo.
	#[must_use]
	pub fn host_timestamp(&self) -> SystemTime {
		UNIX_EPOCH
			.checked_add(Duration::from_secs(u64::from(self.timestamp_on_host)))
			.unwrap_or_else(SystemTime::now)
	}

	#[must_use]
	pub const fn host_pid(&self) -> u32 {
		self.pid_on_host
	}
}

impl TryFrom<Bytes> for PCFSSataPacketHeader {
	type Error = NetworkParseError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x20 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"PCFSSata",
				"Header",
				0x20,
				value.len(),
				value,
			));
		}
		if value.len() > 0x20 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"PCFSSataHeader",
				value.slice(0x20..),
			));
		}

		let packet_data_len = u32::from_be_bytes([value[0], value[1], value[2], value[3]]);
		let packet_id = u32::from_be_bytes([value[4], value[5], value[6], value[7]]);
		let flags = u32::from_be_bytes([value[8], value[9], value[10], value[11]]);
		let version = u32::from_be_bytes([value[12], value[13], value[14], value[15]]);
		let timestamp_on_host = u32::from_be_bytes([value[16], value[17], value[18], value[19]]);
		let pid_on_host = u32::from_be_bytes([value[20], value[21], value[22], value[23]]);

		let should_be_padding = [
			value[24], value[25], value[26], value[27], value[28], value[29], value[30], value[31],
		];
		if should_be_padding != [0x0; 8] {
			return Err(PCFSSataProtocolError::HeaderBadPadding(should_be_padding).into());
		}

		Ok(Self {
			packet_data_len,
			packet_id,
			flags: PCFSSataPacketHeaderFields(flags),
			version,
			timestamp_on_host,
			pid_on_host,
		})
	}
}

const PCFS_SATA_PACKET_HEADER_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("data_len"),
	NamedField::new("id"),
	NamedField::new("flags"),
	NamedField::new("version"),
	NamedField::new("host_timestamp"),
	NamedField::new("host_pid"),
];

impl Structable for PCFSSataPacketHeader {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"PCFSSataPacketHeader",
			Fields::Named(PCFS_SATA_PACKET_HEADER_FIELDS),
		)
	}
}

impl Valuable for PCFSSataPacketHeader {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			PCFS_SATA_PACKET_HEADER_FIELDS,
			&[
				Valuable::as_value(&self.packet_data_len),
				Valuable::as_value(&self.packet_id),
				Valuable::as_value(&self.flags.0),
				Valuable::as_value(&self.version),
				Valuable::as_value(&self.timestamp_on_host),
				Valuable::as_value(&self.pid_on_host),
			],
		));
	}
}

/// All of the potential types of bodies when dealing with a PCFS socket
/// body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PCFSSataPacketBody {
	/// A ping request coming in.
	Ping(PCFSPingPacketBody),
}

impl TryFrom<Bytes> for PCFSSataPacketBody {
	type Error = PCFSSataProtocolError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		// Packet body is too small, there's no packet id :(
		if value.len() < 4 {}
		todo!()
	}
}
