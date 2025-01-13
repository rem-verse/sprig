//! Definitions, and handlers for the `OpenFolder` packet type.
//!
//! This does not iterate over the directory at all, just opens the directory.

use crate::{
	errors::{CatBridgeError, NetworkParseError},
	fsemul::{
		host_filesystem::ResolvedLocation,
		pcfs::sata_proto::{construct_sata_response, SataPacketHeader},
		HostFilesystem,
	},
};
use bytes::{BufMut, Bytes, BytesMut};
use std::ffi::CStr;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;
/// An error code to send when a path does not exist.
///
/// This is also used in some places that are a bit of a stretch like for
/// network shares on disk space. The path doesn't exist on a disk, so this
/// error code is used, even if it's not quite exact.
const PATH_NOT_EXIST_ERROR: u32 = 0xFFF0_FFE9;

/// A packet to open an iterator over a folders contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataOpenFolderPacketBody {
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

impl SataOpenFolderPacketBody {
	#[must_use]
	pub fn path(&self) -> &str {
		self.path.as_str()
	}

	/// Handle opening a folder upon request.
	///
	/// ## Errors
	///
	/// If we cannot construct a sata response packet because our data to send
	/// was somehow too large (this should ideally never happen).
	pub async fn handle(
		&self,
		request_header: &SataPacketHeader,
		host_filesystem: &HostFilesystem,
	) -> Result<Bytes, CatBridgeError> {
		let Ok(final_location) = host_filesystem.resolve_path(&self.path) else {
			return Self::construct_error(request_header, PATH_NOT_EXIST_ERROR);
		};
		let ResolvedLocation::Filesystem(fs_location) = final_location else {
			todo!("network shares not yet implemented!")
		};

		let Ok(fd) = host_filesystem
			.open_folder(fs_location.resolved_path())
			.await
		else {
			return Self::construct_error(request_header, FS_ERROR);
		};

		let mut buff = BytesMut::with_capacity(8);
		buff.put_u32(0);
		buff.put_i32(fd);
		Ok(construct_sata_response(request_header, 0, buff.freeze())?)
	}

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

impl TryFrom<Bytes> for SataOpenFolderPacketBody {
	type Error = NetworkParseError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x200 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataOpenFolder",
				"Body",
				0x200,
				value.len(),
				value,
			));
		}
		if value.len() > 0x200 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataOpenFolder",
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

const SATA_OPEN_FOLDER_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[NamedField::new("path")];

impl Structable for SataOpenFolderPacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataOpenFolderPacketBody",
			Fields::Named(SATA_OPEN_FOLDER_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataOpenFolderPacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_OPEN_FOLDER_PACKET_BODY_FIELDS,
			&[Valuable::as_value(&self.path)],
		));
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use crate::fsemul::host_filesystem::test_helpers::{
		create_temporary_host_filesystem, join_many,
	};

	#[tokio::test]
	pub async fn simple_open_folder_request() {
		let (tempdir, fs) = create_temporary_host_filesystem();
		let request = SataOpenFolderPacketBody {
			path: "/%SLC_EMU_DIR/to-query/".to_owned(),
		};
		let mocked_header = SataPacketHeader {
			packet_data_len: 0,
			packet_id: 0,
			flags: 0,
			version: 0,
			timestamp_on_host: 0,
			pid_on_host: 0,
		};

		let base_dir = join_many(tempdir.path(), ["data", "slc", "to-query"]);
		tokio::fs::create_dir(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");

		let mut response = request
			.handle(&mocked_header, &fs)
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
			&[0x00, 0x00, 0x00, 0x00], // folder handle.
		);
		fs.close_folder(i32::from_be_bytes([
			response[4],
			response[5],
			response[6],
			response[7],
		]))
		.await;
	}
}
