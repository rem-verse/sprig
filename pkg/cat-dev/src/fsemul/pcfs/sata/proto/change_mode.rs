//! Definitions, and handlers for the `ChangeMode` packet type.
//!
//! Although this implies you can set any arbitrary mode, unfortunately because
//! the SDK only supported windows, and windows doesn't have full mode strings,
//! we can only set read only. We're basically a toggle between 0444, and 0666.

use crate::{errors::NetworkParseError, fsemul::pcfs::errors::PCFSApiError};
use bytes::{BufMut, Bytes, BytesMut};
use std::ffi::CStr;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

#[cfg(feature = "servers")]
use crate::{
	errors::{CatBridgeError, FSError},
	fsemul::{
		HostFilesystem,
		host_filesystem::ResolvedLocation,
		pcfs::sata::proto::{SataPacketHeader, construct_sata_response},
	},
};
#[cfg(feature = "servers")]
use std::fs::set_permissions;
#[cfg(feature = "servers")]
use tracing::debug;

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

/// A packet to get change read-only state of a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataChangeModePacketBody {
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
	/// If we're setting the write mode.
	set_write_mode: bool,
}

impl SataChangeModePacketBody {
	/// Attempt to construct a new change mode packet.
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

	/// Update the path to send in this particular change mode packet.
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

	/// Handle a Change Mode Request.
	///
	/// This handles setting read only mode one or off.
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
				packet.typ = "PCFSSrvChangeMode",
				"Failed to resolve path!",
			);

			return Ok(construct_sata_response(
				request_header,
				0,
				Bytes::from(Vec::from(PATH_NOT_EXIST_ERROR.to_be_bytes())),
			)?);
		};
		let ResolvedLocation::Filesystem(fs_location) = final_location else {
			todo!("network shares not yet implemented!")
		};
		// Path doesn't exist.
		if !fs_location.canonicalized_is_exact() {
			debug!(
				packet.path = self.path.as_str(),
				packet.typ = "PCFSSrvChangeMode",
				"Cannot change mode of path that does not exist!",
			);

			return Ok(construct_sata_response(
				request_header,
				0,
				Bytes::from(Vec::from(PATH_NOT_EXIST_ERROR.to_be_bytes())),
			)?);
		}

		let Ok(metadata) = fs_location.closest_resolved_path().metadata() else {
			debug!(
				packet.path = self.path.as_str(),
				packet.typ = "PCFSSrvChangeMode",
				"Failed to get path metadata!",
			);

			return Ok(construct_sata_response(
				request_header,
				0,
				Bytes::from(Vec::from(FS_ERROR.to_be_bytes())),
			)?);
		};

		// If we're changing to write, we need to do a path check.
		let mut perms = metadata.permissions();
		if self.set_write_mode
			&& !host_filesystem.path_allows_writes(fs_location.closest_resolved_path())
		{
			debug!(
				packet.path = self.path.as_str(),
				packet.typ = "PCFSSrvChangeMode",
				"Path cannot become writable!",
			);

			return Ok(construct_sata_response(
				request_header,
				0,
				Bytes::from(Vec::from(FS_ERROR.to_be_bytes())),
			)?);
		}
		perms.set_readonly(!self.set_write_mode);
		// Don't set folders as read-only.
		//
		// Windows 7 (where Cafe-SDK targeted), allows you to create files
		// within a "read only" directory. The SDK depends on this "buggy"
		// behavior. It will set read only attributes on a directory, and then
		// attempt to create files in that directory anyway.
		//
		// Thanks Windows :)
		let result = if fs_location.closest_resolved_path().is_dir() {
			if self.set_write_mode {
				host_filesystem
					.ensure_directory_not_read_only(fs_location.closest_resolved_path())
					.await;
				Ok(())
			} else {
				host_filesystem
					.mark_directory_read_only(fs_location.closest_resolved_path().clone())
					.await
			}
		} else {
			set_permissions(fs_location.closest_resolved_path(), perms).map_err(FSError::IO)
		};

		if result.is_err() {
			debug!(
				cause = ?result,
				packet.path = self.path.as_str(),
				packet.typ = "PCFSSrvChangeMode",
				"Failed to change read-only attribute!",
			);

			return Ok(construct_sata_response(
				request_header,
				0,
				Bytes::from(Vec::from(FS_ERROR.to_be_bytes())),
			)?);
		}

		Ok(construct_sata_response(
			request_header,
			0,
			Bytes::from(vec![0; 4]),
		)?)
	}
}

impl TryFrom<Bytes> for SataChangeModePacketBody {
	type Error = NetworkParseError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x204 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataChangeMode",
				"Body",
				0x204,
				value.len(),
				value,
			));
		}
		if value.len() > 0x204 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataChangeMode",
				value.slice(0x204..),
			));
		}

		let (path_bytes, num) = value.split_at(0x200);
		let path_c_str =
			CStr::from_bytes_until_nul(path_bytes).map_err(NetworkParseError::BadCString)?;
		let write_mode_flags = u32::from_be_bytes([num[0], num[1], num[2], num[3]]);
		let final_path = path_c_str.to_str()?.to_owned();

		Ok(Self {
			path: final_path,
			set_write_mode: write_mode_flags & 0x222 != 0,
		})
	}
}

impl From<&SataChangeModePacketBody> for Bytes {
	fn from(value: &SataChangeModePacketBody) -> Self {
		let mut result = BytesMut::with_capacity(0x204);
		result.extend_from_slice(value.path.as_bytes());
		// These are C Strings so we need a NUL terminator.
		// Pad with `0`, til we get a full path with a nul terminator.
		result.extend(BytesMut::zeroed(0x200 - result.len()));
		result.put_u32(if value.set_write_mode { 0x666 } else { 0x444 });
		result.freeze()
	}
}

impl From<SataChangeModePacketBody> for Bytes {
	fn from(value: SataChangeModePacketBody) -> Self {
		Self::from(&value)
	}
}

const SATA_CHANGE_MODE_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[NamedField::new("path")];

impl Structable for SataChangeModePacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataChangeModePacketBody",
			Fields::Named(SATA_CHANGE_MODE_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataChangeModePacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_CHANGE_MODE_PACKET_BODY_FIELDS,
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
	pub async fn change_mode_request() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
		let request = SataChangeModePacketBody {
			path: "/%SLC_EMU_DIR/to-query/file.txt".to_owned(),
			set_write_mode: false,
		};
		let mocked_header = SataPacketHeader {
			packet_data_len: 0,
			packet_id: 0,
			flags: 0,
			version: 0,
			timestamp_on_host: 0,
			pid_on_host: 0,
		};

		// Okay let's create some files and some sizes in there.
		let base_dir = join_many(tempdir.path(), ["data", "slc", "to-query"]);
		tokio::fs::create_dir(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		tokio::fs::write(join_many(&base_dir, ["file.txt"]), vec![0; 1307])
			.await
			.expect("Failed to write test file!");

		let mut response = request
			.handle(&mocked_header, &fs)
			.await
			.expect("Failed to handle change mode!");
		assert_eq!(response.len(), 4 + 0x20, "Packet is not correct size!");
		// Okay first chop off the header, we don't care.
		_ = response.split_to(0x20);
		assert_eq!(
			response,
			Bytes::from(vec![
				0x00, 0x00, 0x00, 0x00, // RC
			]),
		);
	}
}
