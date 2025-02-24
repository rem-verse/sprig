//! Definitions, and handlers for the `WriteFile` packet type.
//!
//! This is what actively handles writing bytes to a file. With either
//! FFIO, and Combined Send/Recv options being turned on/off.

use crate::{errors::NetworkParseError, fsemul::pcfs::sata::proto::MoveToFileLocation};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

#[cfg(feature = "servers")]
use crate::{
	errors::{CatBridgeError, NetworkError},
	fsemul::{
		HostFilesystem,
		pcfs::sata::proto::{SataPacketHeader, SataServerProtoChunker, construct_sata_response},
	},
};
#[cfg(feature = "servers")]
use futures::{StreamExt, stream::SplitStream};
#[cfg(feature = "servers")]
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering as AtomicOrdering},
};
#[cfg(feature = "servers")]
use tokio::net::TcpStream;
#[cfg(feature = "servers")]
use tokio_util::codec::Framed;
#[cfg(feature = "servers")]
use tracing::debug;

#[cfg(feature = "servers")]
/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;

/// A packet to write to an already open file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataWriteFilePacketBody {
	block_count: u32,
	block_size: u32,
	handle: i32,
	move_to_pointer: MoveToFileLocation,
	should_move: bool,
}

impl SataWriteFilePacketBody {
	/// Create a new write file packet.
	#[must_use]
	pub const fn new(
		block_count: u32,
		block_size: u32,
		file_descriptor: i32,
		move_to: Option<MoveToFileLocation>,
	) -> Self {
		Self {
			block_count,
			block_size,
			handle: file_descriptor,
			move_to_pointer: if let Some(mt) = move_to {
				mt
			} else {
				MoveToFileLocation::Begin
			},
			should_move: move_to.is_some(),
		}
	}

	#[must_use]
	pub const fn block_count(&self) -> u32 {
		self.block_count
	}

	pub const fn set_block_count(&mut self, new_count: u32) {
		self.block_count = new_count;
	}

	#[must_use]
	pub const fn block_size(&self) -> u32 {
		self.block_size
	}

	pub const fn set_block_size(&mut self, new_size: u32) {
		self.block_size = new_size;
	}

	#[must_use]
	pub const fn file_descriptor(&self) -> i32 {
		self.handle
	}

	pub const fn set_file_descriptor(&mut self, new_fd: i32) {
		self.handle = new_fd;
	}

	#[must_use]
	pub const fn move_to_pointer(&self) -> MoveToFileLocation {
		self.move_to_pointer
	}

	pub const fn set_move_to(&mut self, new_move: Option<MoveToFileLocation>) {
		if let Some(nm) = new_move {
			self.move_to_pointer = nm;
			self.should_move = true;
		} else {
			self.move_to_pointer = MoveToFileLocation::Begin;
			self.should_move = false;
		}
	}

	#[must_use]
	pub const fn should_move(&self) -> bool {
		self.should_move
	}

	/// Handle writing to a file that is already open.
	///
	/// ## Errors
	///
	/// If we cannot construct a sata response packet because our data to send
	/// was somehow too large (this should ideally never happen), or if we're
	/// running on a 16 bit system.
	#[cfg(feature = "servers")]
	pub async fn handle(
		&self,
		request_header: &SataPacketHeader,
		host_filesystem: &HostFilesystem,
		ffio_supported: bool,
		socket: &mut SplitStream<Framed<TcpStream, SataServerProtoChunker>>,
		override_ptr: &Arc<AtomicUsize>,
	) -> Result<Bytes, CatBridgeError> {
		if self.should_move {
			match self.move_to_pointer {
				MoveToFileLocation::Begin => {
					if host_filesystem.seek_file(self.handle, true).await.is_err() {
						debug!(
							packet.fd = self.handle,
							packet.typ = "PCFSSrvWriteFile",
							"Failed to seek to beginning of file!",
						);

						return Self::construct_error(request_header, FS_ERROR);
					}
				}
				MoveToFileLocation::Current => {
					// Luckily to move to current, we don't need to move at all.
				}
				MoveToFileLocation::End => {
					if host_filesystem.seek_file(self.handle, false).await.is_err() {
						debug!(
							packet.fd = self.handle,
							packet.typ = "PCFSSrvWriteFile",
							"Failed to seek to end of file!",
						);

						return Self::construct_error(request_header, FS_ERROR);
					}
				}
			}
		}

		if ffio_supported {
			let len_needed = usize::try_from(self.block_count * self.block_size)
				.map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?;
			// Bypass header and such checks...
			override_ptr.store(len_needed, AtomicOrdering::Release);
			let buff = socket
				.next()
				.await
				.ok_or_else(|| NetworkError::ExpectedData)?
				.map_err(NetworkError::IO)?
				.freeze();
			host_filesystem
				.write_file(self.file_descriptor(), buff)
				.await?;

			let mut result = BytesMut::with_capacity(4);
			result.put_u32(self.block_count * self.block_size);
			Ok(construct_sata_response(request_header, 0, result.freeze())?)
		} else {
			todo!("Implement non-FFIO support.")
		}
	}

	#[cfg(feature = "servers")]
	fn construct_error(
		packet_header: &SataPacketHeader,
		error_code: u32,
	) -> Result<Bytes, CatBridgeError> {
		let mut buff = BytesMut::with_capacity(8);
		buff.put_u32(error_code);
		Ok(construct_sata_response(packet_header, 0, buff.freeze())?)
	}
}

impl From<&SataWriteFilePacketBody> for Bytes {
	fn from(value: &SataWriteFilePacketBody) -> Self {
		let mut buff = BytesMut::with_capacity(20);

		buff.put_u32(value.block_count);
		buff.put_u32(value.block_size);
		buff.put_i32(value.handle);
		buff.put_u32(u32::from(value.move_to_pointer));
		// True is 1, False is 0
		buff.put_u32(u32::from(value.should_move));

		buff.freeze()
	}
}

impl From<SataWriteFilePacketBody> for Bytes {
	fn from(value: SataWriteFilePacketBody) -> Self {
		Self::from(&value)
	}
}

impl TryFrom<Bytes> for SataWriteFilePacketBody {
	type Error = NetworkParseError;

	fn try_from(mut value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 20 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataWriteFile",
				"Body",
				20,
				value.len(),
				value,
			));
		}
		if value.len() > 20 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataWriteFile",
				value.slice(20..),
			));
		}

		let block_count = value.get_u32();
		let block_length = value.get_u32();
		let handle = value.get_i32();
		let move_to_ptr = value.get_u32();
		let should_move = value.get_u32();

		Ok(Self {
			block_count,
			block_size: block_length,
			handle,
			move_to_pointer: MoveToFileLocation::try_from(move_to_ptr)?,
			should_move: (should_move & 1) != 0,
		})
	}
}

const SATA_WRITE_FILE_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("block_count"),
	NamedField::new("block_size"),
	NamedField::new("handle"),
	NamedField::new("move_to_pointer"),
	NamedField::new("should_move"),
];

impl Structable for SataWriteFilePacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataWriteFilePacketBody",
			Fields::Named(SATA_WRITE_FILE_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataWriteFilePacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_WRITE_FILE_PACKET_BODY_FIELDS,
			&[
				Valuable::as_value(&self.block_count),
				Valuable::as_value(&self.block_size),
				Valuable::as_value(&self.handle),
				Valuable::as_value(&self.move_to_pointer),
				Valuable::as_value(&self.should_move),
			],
		));
	}
}
