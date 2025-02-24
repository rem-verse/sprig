//! Definitions, and handlers for the `OpenFile` packet type.
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

#[cfg(feature = "servers")]
use crate::{
	errors::CatBridgeError,
	fsemul::{
		HostFilesystem,
		host_filesystem::ResolvedLocation,
		pcfs::sata::proto::{SataCommandInfo, SataPacketHeader, construct_sata_response},
	},
};
#[cfg(feature = "servers")]
use bytes::BufMut;
#[cfg(feature = "servers")]
use tokio::fs::{OpenOptions, set_permissions};
#[cfg(feature = "servers")]
use tracing::{debug, warn};

#[cfg(feature = "servers")]
/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;
#[cfg(feature = "servers")]
/// An error code to send when a path does not exist.
///
/// This is also used in some places that are a bit of a stretch like for
/// network shares on disk space. The path doesn't exist on a disk, so this
/// error code is used, even if it's not quite exact.
const PATH_NOT_EXIST_ERROR: u32 = 0xFFF0_FFE9;

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

	/// Handle opening a file upon request.
	///
	/// ## Errors
	///
	/// If we cannot construct a sata response packet because our data to send
	/// was somehow too large (this should ideally never happen).
	#[allow(
		// Yes clippy, this is what I want, which is why i wrote it.
		clippy::permissions_set_readonly_false,
	)]
	#[cfg(feature = "servers")]
	pub async fn handle(
		&self,
		request_header: &SataPacketHeader,
		command_info: &SataCommandInfo,
		host_filesystem: &HostFilesystem,
	) -> Result<Bytes, CatBridgeError> {
		let Ok(final_location) = host_filesystem.resolve_path(&self.path) else {
			debug!(
				packet.path = self.path.as_str(),
				packet.typ = "PCFSSrvOpenFile",
				"Failed to resolve path!",
			);

			return Self::construct_error(request_header, PATH_NOT_EXIST_ERROR);
		};
		let ResolvedLocation::Filesystem(fs_location) = final_location else {
			todo!("network shares not yet implemented!")
		};

		// Since you can only have one of 'raw', if we want to check for
		// 'a' || 'w', we can instead check for the absence of 'r'.
		if !host_filesystem.path_allows_writes(fs_location.resolved_path())
			&& (!self.mode_string.contains('r') || self.mode_string.contains('+'))
		{
			debug!(
				packet.mode = self.mode_string,
				packet.path = self.path.as_str(),
				packet.typ = "PCFSSrvOpenFile",
				"Path does not allow opening as writable!",
			);

			return Self::construct_error(request_header, FS_ERROR);
		}
		let [allow_becoming_write, _, _, _] = command_info.capabilities().1.to_be_bytes();
		// If it exists, we potentially need to change read only mode flag.
		if fs_location.resolved_path().exists() {
			let Ok(metadata) = fs_location.resolved_path().metadata() else {
				debug!(
					packet.path = self.path.as_str(),
					packet.typ = "PCFSSrvOpenFile",
					"Failed to get metadata of resolved path!",
				);

				return Self::construct_error(request_header, FS_ERROR);
			};
			let mut perms = metadata.permissions();
			if perms.readonly() && !self.mode_string.contains('r') && allow_becoming_write == 0 {
				debug!(
					path.is_read_only = perms.readonly(),
					packet.mode = self.mode_string,
					packet.path = self.path.as_str(),
					packet.typ = "PCFSSrvOpenFile",
					"Path is marked read-only, and mode was requested as non-read!",
				);

				return Self::construct_error(request_header, FS_ERROR);
			}
			perms.set_readonly(false);
			if set_permissions(fs_location.resolved_path(), perms)
				.await
				.is_err()
			{
				debug!(
					packet.path = self.path.as_str(),
					packet.typ = "PCFSSrvOpenFile",
					"Failed to update permissions as requested on open!"
				);

				return Self::construct_error(request_header, FS_ERROR);
			}
		}

		// Okay time to open!
		let mut options = OpenOptions::new();
		if self.mode_string.contains('r') {
			options.read(true);
		}
		if self.mode_string.contains('w') {
			options.write(true).truncate(true).create(true);
		}
		if self.mode_string.contains('a') {
			options.write(true).truncate(false).create(true);
		}
		if self.mode_string.contains('+') {
			options.create(true);
		}

		let fd = match host_filesystem
			.open_file(options, fs_location.resolved_path())
			.await
		{
			Ok(fd) => fd,
			Err(cause) => {
				warn!(
					?cause,
					packet.path = self.path.as_str(),
					packet.typ = "PCFSSrvOpenFile",
					"Failed to open file!",
				);

				return Self::construct_error(request_header, FS_ERROR);
			}
		};

		debug!(
			result.fd = fd,
			packet.path = self.path.as_str(),
			packet.typ = "PCFSSrvOpenFile",
			"Successfully opened file!",
		);
		let mut buff = BytesMut::with_capacity(8);
		buff.put_u32(0);
		buff.put_i32(fd);
		Ok(construct_sata_response(request_header, 0, buff.freeze())?)
	}

	#[cfg(feature = "servers")]
	fn construct_error(
		packet_header: &SataPacketHeader,
		error_code: u32,
	) -> Result<Bytes, CatBridgeError> {
		let mut buff = BytesMut::with_capacity(8);
		buff.put_u32(error_code);
		buff.put_u32(0);

		Ok(construct_sata_response(packet_header, 0, buff.freeze())?)
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

#[cfg(test)]
mod unit_tests {
	#[cfg(feature = "servers")]
	use super::*;
	#[cfg(feature = "servers")]
	use crate::fsemul::host_filesystem::test_helpers::{
		create_temporary_host_filesystem, join_many,
	};

	#[cfg(feature = "servers")]
	#[tokio::test]
	pub async fn simple_open_file_request() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
		let request = SataOpenFilePacketBody {
			path: "/%SLC_EMU_DIR/to-query/file.txt".to_owned(),
			mode_string: "r".to_owned(),
		};
		let mocked_header = SataPacketHeader {
			packet_data_len: 0,
			packet_id: 0,
			flags: 0,
			version: 0,
			timestamp_on_host: 0,
			pid_on_host: 0,
		};
		let mocked_command_info = SataCommandInfo {
			user: (0, 0),
			capabilities: (0, 0),
			command: 0x5,
		};

		let base_dir = join_many(tempdir.path(), ["data", "slc", "to-query"]);
		tokio::fs::create_dir(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		tokio::fs::write(join_many(&base_dir, ["file.txt"]), vec![0; 1307])
			.await
			.expect("Failed to write test file!");

		let mut response = request
			.handle(&mocked_header, &mocked_command_info, &fs)
			.await
			.expect("Failed to handle change mode!");
		assert_eq!(response.len(), 8 + 0x20, "Packet is not correct size!");
		// Okay first chop off the header, we don't care.
		_ = response.split_to(0x20);
		assert_eq!(
			&response[..4],
			&[0x00, 0x00, 0x00, 0x00], // RC
		);
		assert_ne!(
			&response[4..],
			&[0x00, 0x00, 0x00, 0x00], // File handle.
		);
		fs.close_file(i32::from_be_bytes([
			response[4],
			response[5],
			response[6],
			response[7],
		]))
		.await;
	}
}
