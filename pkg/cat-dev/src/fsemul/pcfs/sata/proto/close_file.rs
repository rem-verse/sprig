//! Definitions for the `CloseFile` packet type, and it's response types.
//!
//! This closes an already existing open file given just a file handle.

use crate::errors::NetworkParseError;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// A packet to close a particular file given a file descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataCloseFilePacketBody {
	file_descriptor: i32,
}

impl SataCloseFilePacketBody {
	/// Create a new close file packet.
	#[must_use]
	pub const fn new(fd: i32) -> Self {
		Self {
			file_descriptor: fd,
		}
	}

	#[must_use]
	pub const fn file_descriptor(&self) -> i32 {
		self.file_descriptor
	}

	pub const fn set_file_descriptor(&mut self, new_fd: i32) {
		self.file_descriptor = new_fd;
	}
}

impl TryFrom<Bytes> for SataCloseFilePacketBody {
	type Error = NetworkParseError;

	fn try_from(mut value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x4 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataCloseFile",
				"Body",
				0x4,
				value.len(),
				value,
			));
		}
		if value.len() > 0x4 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataCloseFile",
				value.slice(0x4..),
			));
		}

		let fd = value.get_i32();

		Ok(Self {
			file_descriptor: fd,
		})
	}
}

impl From<&SataCloseFilePacketBody> for Bytes {
	fn from(value: &SataCloseFilePacketBody) -> Self {
		let mut body = BytesMut::with_capacity(4);
		body.put_i32(value.file_descriptor);
		body.freeze()
	}
}

impl From<SataCloseFilePacketBody> for Bytes {
	fn from(value: SataCloseFilePacketBody) -> Self {
		Self::from(&value)
	}
}

const SATA_CLOSE_FILE_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[NamedField::new("fd")];

impl Structable for SataCloseFilePacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataCloseFilePacketBody",
			Fields::Named(SATA_CLOSE_FILE_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataCloseFilePacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_CLOSE_FILE_PACKET_BODY_FIELDS,
			&[Valuable::as_value(&self.file_descriptor)],
		));
	}
}
