//! Definitions, and handlers for the `StatFile` packet type.
//!
//! This doesn't actually "handle" anything itself. As this is the same as
//! `GetInfoByQuery` query type 5. So we just parse as a body, and then in
//! the handler call query type 5.

use crate::errors::NetworkParseError;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// A packet to get information about an open file handle.
///
/// This can do everything from "get the free space of the disk this path
/// is on", to "get my some metadata about this very specific path."
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataStatFilePacketBody {
	file_descriptor: i32,
}

impl SataStatFilePacketBody {
	/// Create a new packet to STAT a particular file.
	#[must_use]
	pub const fn new(file_descriptor: i32) -> Self {
		Self { file_descriptor }
	}

	#[must_use]
	pub const fn file_descriptor(&self) -> i32 {
		self.file_descriptor
	}

	pub const fn set_file_descriptor(&mut self, new_fd: i32) {
		self.file_descriptor = new_fd;
	}
}

impl From<&SataStatFilePacketBody> for Bytes {
	fn from(value: &SataStatFilePacketBody) -> Self {
		let mut buff = BytesMut::with_capacity(4);
		buff.put_i32(value.file_descriptor);
		buff.freeze()
	}
}

impl From<SataStatFilePacketBody> for Bytes {
	fn from(value: SataStatFilePacketBody) -> Self {
		Self::from(&value)
	}
}

impl TryFrom<Bytes> for SataStatFilePacketBody {
	type Error = NetworkParseError;

	fn try_from(mut value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x4 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataStatFile",
				"Body",
				0x4,
				value.len(),
				value,
			));
		}
		if value.len() > 0x4 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataStatFile",
				value.slice(0x4..),
			));
		}

		let fd = value.get_i32();

		Ok(Self {
			file_descriptor: fd,
		})
	}
}

const SATA_STAT_FILE_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[NamedField::new("fd")];

impl Structable for SataStatFilePacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataStatFilePacketBody",
			Fields::Named(SATA_STAT_FILE_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataStatFilePacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_STAT_FILE_PACKET_BODY_FIELDS,
			&[Valuable::as_value(&self.file_descriptor)],
		));
	}
}
