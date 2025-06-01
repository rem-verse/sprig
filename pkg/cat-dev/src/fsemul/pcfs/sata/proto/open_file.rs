//! Definitions for the `OpenFile` packet type, and it's response types.
//!
//! This doesn't _Read_ any data out of the file, but merely opens it for
//! reading, or writing.

use crate::{
	errors::NetworkParseError,
	fsemul::pcfs::errors::{PCFSApiError, SataProtocolError},
};
use bytes::{Bytes, BytesMut};
use std::ffi::CStr;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// A packet to open a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataOpenFilePacketBody {
	/// The current mode string.
	mode_string: String,
	/// The path to query, note that this is not the 'resolved' path which is
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

impl SataOpenFilePacketBody {
	/// Attempt to construct a new open file packet.
	///
	/// ## Errors
	///
	/// If the path is longer than 511 bytes. Normally the max path is 512 bytes,
	/// but because we need to encode our data as a C-String with a NUL
	/// terminator we cannot be longer than 511 bytes.
	///
	/// Consider using relative/mapped paths if possible when dealing with long
	/// paths.
	///
	/// We will also error if the mode string is not valid, it requires to be in
	/// the format of: `(r|w|a)(b+)+` (e.g. starts with either 'rwa', and then
	/// has either `b`, or `+` after that one or more times).
	pub fn new(path: String, mode_string: String) -> Result<Self, PCFSApiError> {
		if path.len() > 511 {
			return Err(PCFSApiError::PathTooLong(path));
		}
		for (idx, car) in mode_string.chars().enumerate() {
			if idx == 0 && !['r', 'w', 'a'].contains(&car) {
				return Err(PCFSApiError::BadModeString(mode_string));
			}
			if idx > 2 {
				return Err(PCFSApiError::BadModeString(mode_string));
			}
			if idx != 0 && !['b', '+'].contains(&car) {
				return Err(PCFSApiError::BadModeString(mode_string));
			}
		}

		Ok(Self { mode_string, path })
	}

	#[must_use]
	pub fn mode(&self) -> &str {
		self.mode_string.as_str()
	}

	/// Update the path to send in this particular create directory packet.
	///
	/// ## Errors
	///
	/// We will also error if the mode string is not valid, it requires to be in
	/// the format of: `(r|w|a)(b+)+` (e.g. starts with either 'rwa', and then
	/// has either `b`, or `+` after that one or more times).
	pub fn set_mode(&mut self, new_mode: String) -> Result<(), PCFSApiError> {
		for (idx, car) in new_mode.chars().enumerate() {
			if idx == 0 && !['r', 'w', 'a'].contains(&car) {
				return Err(PCFSApiError::BadModeString(new_mode));
			}
			if idx > 2 {
				return Err(PCFSApiError::BadModeString(new_mode));
			}
			if idx != 0 && !['b', '+'].contains(&car) {
				return Err(PCFSApiError::BadModeString(new_mode));
			}
		}

		self.mode_string = new_mode;
		Ok(())
	}

	#[must_use]
	pub fn path(&self) -> &str {
		self.path.as_str()
	}

	/// Update the path to send in this particular open file packet.
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

impl From<&SataOpenFilePacketBody> for Bytes {
	fn from(value: &SataOpenFilePacketBody) -> Self {
		let mut result = BytesMut::with_capacity(0x210);
		result.extend_from_slice(value.mode_string.as_bytes());
		// Our mode strings are always shorter than the fully padded,
		// even when you include we need to add a NUL terminator.
		//
		// Since NUL terminator and our padding at the same, just pad
		// all at once.
		result.extend(BytesMut::with_capacity(0x10 - result.len()));
		result.extend_from_slice(value.path.as_bytes());
		// These are C Strings so we need a NUL terminator.
		// Pad with `0`, til we get a full path with a nul terminator.
		result.extend(BytesMut::zeroed(0x210 - result.len()));
		result.freeze()
	}
}

impl From<SataOpenFilePacketBody> for Bytes {
	fn from(value: SataOpenFilePacketBody) -> Self {
		Self::from(&value)
	}
}

impl TryFrom<Bytes> for SataOpenFilePacketBody {
	type Error = NetworkParseError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x210 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataOpenFile",
				"Body",
				0x210,
				value.len(),
				value,
			));
		}
		if value.len() > 0x210 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataOpenFile",
				value.slice(0x210..),
			));
		}

		let (mode_bytes, path_bytes) = value.split_at(0x10);
		let mode_c_str =
			CStr::from_bytes_until_nul(mode_bytes).map_err(NetworkParseError::BadCString)?;
		let path_c_str =
			CStr::from_bytes_until_nul(path_bytes).map_err(NetworkParseError::BadCString)?;
		let final_mode = mode_c_str.to_str()?.to_owned();
		for (idx, car) in final_mode.chars().enumerate() {
			if idx == 0 && !['r', 'w', 'a'].contains(&car) {
				return Err(SataProtocolError::BadModeString(final_mode).into());
			}
			if idx > 2 {
				return Err(SataProtocolError::BadModeString(final_mode).into());
			}
			if idx != 0 && !['b', '+'].contains(&car) {
				return Err(SataProtocolError::BadModeString(final_mode).into());
			}
		}

		Ok(Self {
			mode_string: final_mode,
			path: path_c_str.to_str()?.to_owned(),
		})
	}
}

const SATA_OPEN_FILE_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[NamedField::new("path")];

impl Structable for SataOpenFilePacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataOpenFilePacketBody",
			Fields::Named(SATA_OPEN_FILE_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataOpenFilePacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_OPEN_FILE_PACKET_BODY_FIELDS,
			&[Valuable::as_value(&self.path)],
		));
	}
}
