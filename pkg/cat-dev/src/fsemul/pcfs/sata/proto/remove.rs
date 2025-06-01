//! Definitions for the `Remove` packet type, and it's response types.
//!
//! This does destructive things, and removes files from your filesystem. You
//! can disable the behavior of truly "removing" items from your filesystem,
//! and configure your PCFS client to just move files to `.rm`

use crate::{errors::NetworkParseError, fsemul::pcfs::errors::PCFSApiError};
use bytes::{Bytes, BytesMut};
use std::ffi::CStr;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// A packet to remove a file/directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataRemovePacketBody {
	/// The path to remove, note that this is not the 'resolved' path which is
	/// the path to actual read from.
	///
	/// Interpolation has a few known ways of being replaced:
	///
	/// - `%MLC_EMU_DIR`: `<cafe_sdk>/data/mlc/`
	/// - `%SLC_EMU_DIR`: `<cafe_sdk>/data/slc/`
	/// - `%DISC_EMU_DIR`: `<cafe_sdk>/data/disc/`
	/// - `%SAVE_EMU_DIR`: `<cafe_sdk>/data/save/`
	/// - `%NETWORK`: <mounted network share path>
	path: String,
}

impl SataRemovePacketBody {
	/// Attempt to construct a new remove file packet.
	///
	/// ## Errors
	///
	/// If the path is longer than 511 bytes. Normally the max path is 512 bytes,
	/// but because we need to encode our data as a C-String with a NUL
	/// terminator we cannot be longer than 511 bytes.
	///
	/// Consider using relative/mapped paths if possible when dealing with long
	/// paths.
	pub fn new(path: String) -> Result<Self, PCFSApiError> {
		if path.len() > 511 {
			return Err(PCFSApiError::PathTooLong(path));
		}

		Ok(Self { path })
	}

	#[must_use]
	pub fn path(&self) -> &str {
		self.path.as_str()
	}

	/// Update the path to send in this particular remove file packet.
	///
	/// ## Errors
	///
	/// If the path is longer than 511 bytes. Normally the max path is 512 bytes,
	/// but because we need to encode our data as a C-String with a NUL
	/// terminator we cannot be longer than 511 bytes.
	///
	/// Consider using relative/mapped paths if possible when dealing with long
	/// paths.
	pub fn set_path(&mut self, new_path: String) -> Result<(), PCFSApiError> {
		if new_path.len() > 511 {
			return Err(PCFSApiError::PathTooLong(new_path));
		}

		self.path = new_path;
		Ok(())
	}
}

impl From<&SataRemovePacketBody> for Bytes {
	fn from(value: &SataRemovePacketBody) -> Self {
		let mut result = BytesMut::with_capacity(0x200);
		result.extend_from_slice(value.path.as_bytes());
		// These are C Strings so we need a NUL terminator.
		// Pad with `0`, til we get a full path with a nul terminator.
		result.extend(BytesMut::zeroed(0x200 - result.len()));
		result.freeze()
	}
}

impl From<SataRemovePacketBody> for Bytes {
	fn from(value: SataRemovePacketBody) -> Self {
		Self::from(&value)
	}
}

impl TryFrom<Bytes> for SataRemovePacketBody {
	type Error = NetworkParseError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x200 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataRemove",
				"Body",
				0x200,
				value.len(),
				value,
			));
		}
		if value.len() > 0x200 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataRemove",
				value.slice(0x200..),
			));
		}

		let path_c_str =
			CStr::from_bytes_until_nul(&value).map_err(NetworkParseError::BadCString)?;

		Ok(Self {
			path: path_c_str.to_str()?.to_owned(),
		})
	}
}

const SATA_REMOVE_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[NamedField::new("path")];

impl Structable for SataRemovePacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataRemovePacketBody",
			Fields::Named(SATA_REMOVE_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataRemovePacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_REMOVE_PACKET_BODY_FIELDS,
			&[Valuable::as_value(&self.path)],
		));
	}
}
