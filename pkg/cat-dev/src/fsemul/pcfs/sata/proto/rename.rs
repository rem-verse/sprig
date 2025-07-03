//! Definitions for the `Rename` packet type, and it's response types.
//!
//! This moves a file from one place to another as necessary.

use crate::{errors::NetworkParseError, fsemul::pcfs::errors::PCFSApiError};
use bytes::{Bytes, BytesMut};
use std::ffi::CStr;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// A packet to rename an arbitrary path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataRenamePacketBody {
	/// The source path which will be renamed.
	source_path: String,
	/// The path, aka where the final path will be.
	dest_path: String,
}

impl SataRenamePacketBody {
	/// Attempt to construct a new rename packet body.
	///
	/// ## Errors
	///
	/// If any path is longer than 511 bytes. Normally the max path is 512 bytes,
	/// but because we need to encode our data as a C-String with a NUL
	/// terminator we cannot be longer than 511 bytes.
	///
	/// Consider using relative/mapped paths if possible when dealing with long
	/// paths.
	pub fn new(source_path: String, dest_path: String) -> Result<Self, PCFSApiError> {
		if source_path.len() > 511 {
			return Err(PCFSApiError::PathTooLong(source_path));
		}
		if dest_path.len() > 511 {
			return Err(PCFSApiError::PathTooLong(dest_path));
		}

		Ok(Self {
			source_path,
			dest_path,
		})
	}

	#[must_use]
	pub fn source_path(&self) -> &str {
		self.source_path.as_str()
	}

	#[must_use]
	pub fn dest_path(&self) -> &str {
		self.dest_path.as_str()
	}

	/// Update the source path to send in this rename packet directory.
	///
	/// ## Errors
	///
	/// If the path is longer than 511 bytes. Normally the max path is 512 bytes,
	/// but because we need to encode our data as a C-String with a NUL
	/// terminator we cannot be longer than 511 bytes.
	///
	/// Consider using relative/mapped paths if possible when dealing with long
	/// paths.
	pub fn set_source_path(&mut self, new_path: String) -> Result<(), PCFSApiError> {
		if new_path.len() > 511 {
			return Err(PCFSApiError::PathTooLong(new_path));
		}

		self.source_path = new_path;
		Ok(())
	}

	/// Update the destination path to send in this rename packet directory.
	///
	/// ## Errors
	///
	/// If the path is longer than 511 bytes. Normally the max path is 512 bytes,
	/// but because we need to encode our data as a C-String with a NUL
	/// terminator we cannot be longer than 511 bytes.
	///
	/// Consider using relative/mapped paths if possible when dealing with long
	/// paths.
	pub fn set_dest_path(&mut self, new_path: String) -> Result<(), PCFSApiError> {
		if new_path.len() > 511 {
			return Err(PCFSApiError::PathTooLong(new_path));
		}

		self.dest_path = new_path;
		Ok(())
	}
}

impl TryFrom<Bytes> for SataRenamePacketBody {
	type Error = NetworkParseError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x400 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataRenamePacketBody",
				"Body",
				0x400,
				value.len(),
				value,
			));
		}
		if value.len() > 0x400 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataRenamePacketBody",
				value.slice(0x400..),
			));
		}

		let source_path_c_str =
			CStr::from_bytes_until_nul(&value[..0x200]).map_err(NetworkParseError::BadCString)?;
		let dest_path_c_str =
			CStr::from_bytes_until_nul(&value[0x200..]).map_err(NetworkParseError::BadCString)?;

		Ok(Self {
			source_path: source_path_c_str.to_str()?.to_owned(),
			dest_path: dest_path_c_str.to_str()?.to_owned(),
		})
	}
}

impl From<&SataRenamePacketBody> for Bytes {
	fn from(value: &SataRenamePacketBody) -> Self {
		let mut result = BytesMut::with_capacity(0x400);
		result.extend_from_slice(value.source_path.as_bytes());
		// These are C Strings so we need a NUL terminator.
		// Pad with `0`, til we get a full path with a nul terminator.
		result.extend(BytesMut::zeroed(0x200 - result.len()));
		result.extend_from_slice(value.dest_path.as_bytes());
		// Pad again....
		result.extend(BytesMut::zeroed(0x400 - result.len()));
		result.freeze()
	}
}

impl From<SataRenamePacketBody> for Bytes {
	fn from(value: SataRenamePacketBody) -> Self {
		Self::from(&value)
	}
}

const SATA_RENAME_PACKET_BODY_FIELDS: &[NamedField<'static>] =
	&[NamedField::new("source_path"), NamedField::new("dest_path")];

impl Structable for SataRenamePacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataRenamePacketBody",
			Fields::Named(SATA_RENAME_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataRenamePacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_RENAME_PACKET_BODY_FIELDS,
			&[
				Valuable::as_value(&self.source_path),
				Valuable::as_value(&self.dest_path),
			],
		));
	}
}
