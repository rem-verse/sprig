//! Definitions for the `CreateDir` packet type, and it's response types.
//!
//! This creates folders on disk, and nothing more.

use crate::{errors::NetworkParseError, fsemul::pcfs::errors::PCFSApiError};
use bytes::{BufMut, Bytes, BytesMut};
use std::ffi::CStr;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// A packet to create a new directory.
///
/// This will create a directory if it does not exist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataCreateFolderPacketBody {
	/// The path to create, note that this is not the 'resolved' path which is
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
	/// If we're setting the write mode.
	set_write_mode: bool,
}

impl SataCreateFolderPacketBody {
	/// Attempt to construct a new create directory packet.
	///
	/// ## Errors
	///
	/// If the path is longer than 511 bytes. Normally the max path is 512 bytes,
	/// but because we need to encode our data as a C-String with a NUL
	/// terminator we cannot be longer than 511 bytes.
	///
	/// Consider using relative/mapped paths if possible when dealing with long
	/// paths.
	pub fn new(path: String, set_write_mode: bool) -> Result<Self, PCFSApiError> {
		if path.len() > 511 {
			return Err(PCFSApiError::PathTooLong(path));
		}

		Ok(Self {
			path,
			set_write_mode,
		})
	}

	#[must_use]
	pub fn path(&self) -> &str {
		self.path.as_str()
	}

	/// Update the path to send in this particular create directory packet.
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

	#[must_use]
	pub const fn will_set_write_mode(&self) -> bool {
		self.set_write_mode
	}

	/// Update the `set_write_mode` flag, which determines if we'll set the file
	/// as writable or not.
	pub const fn set_write_mode(&mut self, will_set: bool) {
		self.set_write_mode = will_set;
	}
}

impl TryFrom<Bytes> for SataCreateFolderPacketBody {
	type Error = NetworkParseError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x204 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataCreateDirectory",
				"Body",
				0x204,
				value.len(),
				value,
			));
		}
		if value.len() > 0x204 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataCreateDirectory",
				value.slice(0x204..),
			));
		}

		let path_c_str =
			CStr::from_bytes_until_nul(&value[..0x200]).map_err(NetworkParseError::BadCString)?;
		let mode = u32::from_be_bytes([value[0x200], value[0x201], value[0x202], value[0x203]]);

		Ok(Self {
			path: path_c_str.to_str()?.to_owned(),
			set_write_mode: mode & 0x222 != 0,
		})
	}
}

impl From<&SataCreateFolderPacketBody> for Bytes {
	fn from(value: &SataCreateFolderPacketBody) -> Self {
		let mut result = BytesMut::with_capacity(0x204);
		result.extend_from_slice(value.path.as_bytes());
		// These are C Strings so we need a NUL terminator.
		// Pad with `0`, til we get a full path with a nul terminator.
		result.extend(BytesMut::zeroed(0x200 - result.len()));
		result.put_u32(if value.set_write_mode { 0x666 } else { 0x444 });
		result.freeze()
	}
}

impl From<SataCreateFolderPacketBody> for Bytes {
	fn from(value: SataCreateFolderPacketBody) -> Self {
		Self::from(&value)
	}
}

const SATA_CREATE_FOLDER_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[NamedField::new("path")];

impl Structable for SataCreateFolderPacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataCreateDirectoryPacketBody",
			Fields::Named(SATA_CREATE_FOLDER_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataCreateFolderPacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_CREATE_FOLDER_PACKET_BODY_FIELDS,
			&[Valuable::as_value(&self.path)],
		));
	}
}
