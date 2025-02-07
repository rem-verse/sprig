//! Definitions, and handlers for the `CreateDir` packet type.
//!
//! This creates folders on disk, and nothing more.

use crate::errors::NetworkParseError;
use bytes::Bytes;
use std::ffi::CStr;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

#[cfg(feature = "servers")]
use crate::{
	errors::CatBridgeError,
	fsemul::{
		host_filesystem::ResolvedLocation,
		pcfs::sata_proto::{construct_sata_response, SataPacketHeader},
		HostFilesystem,
	},
};
#[cfg(feature = "servers")]
use bytes::{BufMut, BytesMut};
#[cfg(feature = "servers")]
use tokio::fs::{create_dir_all, set_permissions};
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
	#[must_use]
	pub fn path(&self) -> &str {
		self.path.as_str()
	}
	#[must_use]
	pub const fn set_write_mode(&self) -> bool {
		self.set_write_mode
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
		// Mark as read only.
		if !self.set_write_mode {
			let Ok(metadata) = fs_location.resolved_path().metadata() else {
				debug!(
					packet.path = self.path.as_str(),
					packet.typ = "PCFSSrvCreateDirectory",
					"Failed to get paths metadata!",
				);
				return Self::construct_error(request_header, FS_ERROR);
			};
			let mut perms = metadata.permissions();
			perms.set_readonly(true);
			if set_permissions(fs_location.resolved_path(), perms)
				.await
				.is_err()
			{
				debug!(
					packet.path = self.path.as_str(),
					packet.typ = "PCFSSrvCreateDirectory",
					"Failed to update path permissions!",
				);
				return Self::construct_error(request_header, FS_ERROR);
			}
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
