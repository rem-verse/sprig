//! Definitions, and handlers for the `WriteFile` packet type.
//!
//! This is what actively handles writing bytes to a file. With either
//! FFIO, and Combined Send/Recv options being turned on/off.

use crate::{
	errors::{CatBridgeError, NetworkError, NetworkParseError},
	fsemul::{
		pcfs::sata_proto::{
			construct_sata_response, MoveToFileLocation, SataPacketHeader, SataProtoChunker,
		},
		HostFilesystem,
	},
};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures::{stream::SplitStream, StreamExt};
use std::sync::{
	atomic::{AtomicUsize, Ordering as AtomicOrdering},
	Arc,
};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

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
	#[must_use]
	pub const fn block_count(&self) -> u32 {
		self.block_count
	}
	#[must_use]
	pub const fn block_size(&self) -> u32 {
		self.block_size
	}
	#[must_use]
	pub const fn file_descriptor(&self) -> i32 {
		self.handle
	}
	#[must_use]
	pub const fn move_to_pointer(&self) -> MoveToFileLocation {
		self.move_to_pointer
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
	pub async fn handle(
		&self,
		request_header: &SataPacketHeader,
		host_filesystem: &HostFilesystem,
		ffio_supported: bool,
		socket: &mut SplitStream<Framed<TcpStream, SataProtoChunker>>,
		override_ptr: &Arc<AtomicUsize>,
	) -> Result<Bytes, CatBridgeError> {
		if self.should_move {
			match self.move_to_pointer {
				MoveToFileLocation::Begin => {
					if host_filesystem.seek_file(self.handle, true).await.is_err() {
						return Self::construct_error(request_header, FS_ERROR);
					}
				}
				MoveToFileLocation::Current => {
					// Luckily to move to current, we don't need to move at all.
				}
				MoveToFileLocation::End => {
					if host_filesystem.seek_file(self.handle, false).await.is_err() {
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

	fn construct_error(
		packet_header: &SataPacketHeader,
		error_code: u32,
	) -> Result<Bytes, CatBridgeError> {
		let mut buff = BytesMut::with_capacity(8);
		buff.put_u32(error_code);
		Ok(construct_sata_response(packet_header, 0, buff.freeze())?)
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
