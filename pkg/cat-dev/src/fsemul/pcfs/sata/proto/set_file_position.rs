//! Definitions for the `SetFilePosition` packet type, and it's response types.
//!
//! This can change the location of an already open file, similar to what you'd
//! do before reading a file or writing a file.

use crate::{errors::NetworkParseError, fsemul::pcfs::sata::proto::MoveToFileLocation};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// A packet to set the position of an already open file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataSetFilePositionPacketBody {
	handle: i32,
	move_to_pointer: MoveToFileLocation,
}

impl SataSetFilePositionPacketBody {
	/// Create a new set file position packet.
	#[must_use]
	pub const fn new(file_descriptor: i32, move_to: MoveToFileLocation) -> Self {
		Self {
			handle: file_descriptor,
			move_to_pointer: move_to,
		}
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

	pub const fn set_move_to_pointer(&mut self, new_move: MoveToFileLocation) {
		self.move_to_pointer = new_move;
	}
}

impl From<&SataSetFilePositionPacketBody> for Bytes {
	fn from(value: &SataSetFilePositionPacketBody) -> Self {
		let mut buff = BytesMut::with_capacity(8);

		buff.put_i32(value.handle);
		buff.put_u32(u32::from(value.move_to_pointer));

		buff.freeze()
	}
}

impl From<SataSetFilePositionPacketBody> for Bytes {
	fn from(value: SataSetFilePositionPacketBody) -> Self {
		Self::from(&value)
	}
}

impl TryFrom<Bytes> for SataSetFilePositionPacketBody {
	type Error = NetworkParseError;

	fn try_from(mut value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 8 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataSetFilePosition",
				"Body",
				8,
				value.len(),
				value,
			));
		}
		// Packets _can_ come with an extra padded 4 bytes...
		if value.len() > 12 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataSetFilePosition",
				value.slice(12..),
			));
		}

		let handle = value.get_i32();
		let move_to_ptr = value.get_u32();

		Ok(Self {
			handle,
			move_to_pointer: MoveToFileLocation::try_from(move_to_ptr)?,
		})
	}
}

const SATA_SET_FILE_POSITION_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("handle"),
	NamedField::new("move_to_pointer"),
];

impl Structable for SataSetFilePositionPacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataSetFilePositionPacketBody",
			Fields::Named(SATA_SET_FILE_POSITION_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataSetFilePositionPacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_SET_FILE_POSITION_PACKET_BODY_FIELDS,
			&[
				Valuable::as_value(&self.handle),
				Valuable::as_value(&self.move_to_pointer),
			],
		));
	}
}
