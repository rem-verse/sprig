//! Definitions, and handlers for the `CreateDir` packet type.
//!
//! This creates folders on disk, and nothing more.

use crate::{errors::NetworkParseError, fsemul::pcfs::errors::PCFSApiError};
use bytes::{BufMut, Bytes, BytesMut};
use std::ffi::CStr;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

#[cfg(feature = "servers")]
use crate::{
	errors::CatBridgeError,
	fsemul::{
		HostFilesystem,
		host_filesystem::ResolvedLocation,
		pcfs::sata::proto::{SataPacketHeader, construct_sata_response},
	},
};
#[cfg(feature = "servers")]
use tokio::fs::create_dir_all;
#[cfg(feature = "servers")]
use tracing::{debug, error};

#[cfg(feature = "servers")]
/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;

/// A packet to create a new directory.
///
/// This will create a directory if it does not exist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataCreateDirectoryPacketBody {
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

impl SataCreateDirectoryPacketBody {
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

	/// Handle creating a directory upon request.
	///
	/// ## Errors
	///
	/// If we cannot construct a sata response packet because our data to send
	/// was somehow too large (this should ideally never happen).
	#[cfg(feature = "servers")]
	pub async fn handle(
		&self,
		request_header: &SataPacketHeader,
		host_filesystem: &HostFilesystem,
	) -> Result<Bytes, CatBridgeError> {
		let Ok(final_location) = host_filesystem.resolve_path(&self.path) else {
			debug!(
				packet.path = self.path.as_str(),
				packet.typ = "PCFSSrvCreateDirectory",
				"Failed to resolve path!",
			);

			return Self::construct_error(request_header, FS_ERROR);
		};
		let ResolvedLocation::Filesystem(fs_location) = final_location else {
			todo!("network shares not yet implemented!")
		};

		// Do path check for writes.
		if self.set_write_mode
			&& !host_filesystem.path_allows_writes(fs_location.closest_resolved_path())
		{
			debug!(
				packet.path = self.path.as_str(),
				packet.typ = "PCFSSrvCreateDirectory",
				"Cannot create directory in read-only path!",
			);
			return Self::construct_error(request_header, FS_ERROR);
		}

		if !fs_location.resolved_path().exists() {
			if let Err(cause) = create_dir_all(fs_location.resolved_path()).await {
				error!(
				  ?cause,
				  path = %fs_location.resolved_path().display(),
				  "Failed to create directory for PCFS.",
				);
				return Self::construct_error(request_header, FS_ERROR);
			}
		}

		// Don't set folders as read-only.
		//
		// Windows 7 (where Cafe-SDK targeted), allows you to create files
		// within a "read only" directory. The SDK depends on this "buggy"
		// behavior. It will set read only attributes on a directory, and then
		// attempt to create files in that directory anyway.
		//
		// Thanks Windows :)
		if self.set_write_mode {
			host_filesystem
				.ensure_directory_not_read_only(fs_location.resolved_path())
				.await;
		} else if let Err(cause) = host_filesystem
			.mark_directory_read_only(fs_location.resolved_path().clone())
			.await
		{
			error!(
			  ?cause,
			  path = %fs_location.resolved_path().display(),
			  "Failed to mark directory as read-only for PCFS.",
			);
			return Self::construct_error(request_header, FS_ERROR);
		}

		Ok(construct_sata_response(
			request_header,
			0,
			BytesMut::zeroed(4).freeze(),
		)?)
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

impl TryFrom<Bytes> for SataCreateDirectoryPacketBody {
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

impl From<&SataCreateDirectoryPacketBody> for Bytes {
	fn from(value: &SataCreateDirectoryPacketBody) -> Self {
		let mut result = BytesMut::with_capacity(0x204);
		result.extend_from_slice(value.path.as_bytes());
		// These are C Strings so we need a NUL terminator.
		// Pad with `0`, til we get a full path with a nul terminator.
		result.extend(BytesMut::zeroed(0x200 - result.len()));
		result.put_u32(if value.set_write_mode { 0x666 } else { 0x444 });
		result.freeze()
	}
}

impl From<SataCreateDirectoryPacketBody> for Bytes {
	fn from(value: SataCreateDirectoryPacketBody) -> Self {
		Self::from(&value)
	}
}

const SATA_CREATE_DIRECTORY_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[NamedField::new("path")];

impl Structable for SataCreateDirectoryPacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataCreateDirectoryPacketBody",
			Fields::Named(SATA_CREATE_DIRECTORY_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataCreateDirectoryPacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_CREATE_DIRECTORY_PACKET_BODY_FIELDS,
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
	pub async fn test_simple_create_directory() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;

		let base_dir = join_many(tempdir.path(), ["a", "b", "c"]);

		let request = SataCreateDirectoryPacketBody {
			path: base_dir
				.to_str()
				.expect("Test paths must be UTF-8")
				.to_owned(),
			set_write_mode: true,
		};
		let mocked_header = SataPacketHeader {
			packet_data_len: 0,
			packet_id: 0,
			flags: 0,
			version: 0,
			timestamp_on_host: 0,
			pid_on_host: 0,
		};

		assert!(!base_dir.exists());
		let _ = request
			.handle(&mocked_header, &fs)
			.await
			.expect("Failed to handle create directory!");
		assert!(base_dir.exists());
	}
}
