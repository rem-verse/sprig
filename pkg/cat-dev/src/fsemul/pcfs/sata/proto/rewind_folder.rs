//! Definitions for the `RewindDirectory` packet type, and it's response types.
//!
//! This will return the directory iterator to the very beginning of the
//! directory regardless of where it is.

use crate::errors::NetworkParseError;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// A packet to rewind to the beginning of a directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataRewindFolderPacketBody {
	file_descriptor: i32,
}

impl SataRewindFolderPacketBody {
	/// Create a new rewind directory packet.
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

impl From<&SataRewindFolderPacketBody> for Bytes {
	fn from(value: &SataRewindFolderPacketBody) -> Self {
		let mut buff = BytesMut::with_capacity(4);
		buff.put_i32(value.file_descriptor);
		buff.freeze()
	}
}

impl From<SataRewindFolderPacketBody> for Bytes {
	fn from(value: SataRewindFolderPacketBody) -> Self {
		Self::from(&value)
	}
}

impl TryFrom<Bytes> for SataRewindFolderPacketBody {
	type Error = NetworkParseError;

	fn try_from(mut value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x4 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataRewindDir",
				"Body",
				0x4,
				value.len(),
				value,
			));
		}
		if value.len() > 0x4 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataRewindDir",
				value.slice(0x4..),
			));
		}

		let fd = value.get_i32();

		Ok(Self {
			file_descriptor: fd,
		})
	}
}

const SATA_REWIND_FOLDER_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[NamedField::new("fd")];

impl Structable for SataRewindFolderPacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataRewindDirPacketBody",
			Fields::Named(SATA_REWIND_FOLDER_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataRewindFolderPacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_REWIND_FOLDER_PACKET_BODY_FIELDS,
			&[Valuable::as_value(&self.file_descriptor)],
		));
	}
}
