//! Definitions, and handlers for the `ReadDirectory` packet type.
//!
//! This will return the file information for the next file present within a
//! directory. This does not recurse.

use crate::errors::NetworkParseError;
use bytes::{Buf, Bytes};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

#[cfg(feature = "servers")]
use crate::{
	errors::CatBridgeError,
	fsemul::{
		host_filesystem::HostFilesystem,
		pcfs::sata_proto::{
			construct_sata_response, SataGetInfoByQueryPacketBody, SataPacketHeader,
		},
	},
};
#[cfg(feature = "servers")]
use bytes::{BufMut, BytesMut};
#[cfg(feature = "servers")]
use std::path::PathBuf;
#[cfg(feature = "servers")]
use tracing::debug;

#[cfg(feature = "servers")]
/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;
#[cfg(feature = "servers")]
/// No more items in this directory! sorry!
const NO_MORE_ITEMS: u32 = 0xFFF0_FFFC;

/// A packet to get information about another file within a directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataReadDirPacketBody {
	file_descriptor: i32,
}

impl SataReadDirPacketBody {
	#[must_use]
	pub const fn file_descriptor(&self) -> i32 {
		self.file_descriptor
	}

	/// Create a packet to then get the file information in the next file in a
	/// folder.
	///
	/// ## Errors
	///
	/// If we cannot construct a sata response packet, which shouldn't ever happen.
	#[cfg(feature = "servers")]
	pub async fn handle(
		&self,
		request_header: &SataPacketHeader,
		host_filesystem: &HostFilesystem,
	) -> Result<Bytes, CatBridgeError> {
		let Ok(optional_next_item) = host_filesystem.next_in_folder(self.file_descriptor).await
		else {
			debug!(
				packet.fd = self.file_descriptor,
				packet.typ = "PCFSSrvReadDirectory",
				"Failed to query for next item in folder!",
			);

			return Self::construct_error_repsonse(request_header, FS_ERROR);
		};
		let Some((item, components_to_remove)) = optional_next_item else {
			debug!(
				packet.fd = self.file_descriptor,
				packet.typ = "PCFSSrvReadDirectory",
				"No more items in directory!",
			);

			return Self::construct_error_repsonse(request_header, NO_MORE_ITEMS);
		};
		let Ok(info) = SataGetInfoByQueryPacketBody::info_for_path(&item) else {
			debug!(
				packet.fd = self.file_descriptor,
				packet.typ = "PCFSSrvReadDirectory",
				"Failed to get information for next file in folder!",
			);

			return Self::construct_error_repsonse(request_header, FS_ERROR);
		};
		let utf8 = item
			.components()
			.skip(components_to_remove)
			.collect::<PathBuf>()
			.to_string_lossy()
			.to_string();
		if utf8.len() > 255 {
			debug!(
				packet.fd = self.file_descriptor,
				packet.typ = "PCFSSrvReadDirectory",
				"UTF-8 path is too long, cant serve!",
			);

			return Self::construct_error_repsonse(request_header, FS_ERROR);
		}
		let byte_len = utf8.len();

		let mut buff = BytesMut::with_capacity(0x158);
		buff.put_u32(0);
		buff.extend(info);
		buff.extend(utf8.bytes());
		buff.extend(vec![0; 256 - byte_len]);

		Ok(construct_sata_response(request_header, 0, buff.freeze())?)
	}

	#[cfg(feature = "servers")]
	fn construct_error_repsonse(
		request_header: &SataPacketHeader,
		error_code: u32,
	) -> Result<Bytes, CatBridgeError> {
		let mut buff = BytesMut::with_capacity(0x158);
		buff.put_u32(error_code);
		buff.extend_from_slice(&[0; 0x154]);
		Ok(construct_sata_response(request_header, 0, buff.freeze())?)
	}
}

impl TryFrom<Bytes> for SataReadDirPacketBody {
	type Error = NetworkParseError;

	fn try_from(mut value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x4 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataReadDir",
				"Body",
				0x4,
				value.len(),
				value,
			));
		}
		if value.len() > 0x4 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataReadDir",
				value.slice(0x4..),
			));
		}

		let fd = value.get_i32();

		Ok(Self {
			file_descriptor: fd,
		})
	}
}

const SATA_READ_DIRECTORY_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[NamedField::new("fd")];

impl Structable for SataReadDirPacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataReadDirPacketBody",
			Fields::Named(SATA_READ_DIRECTORY_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataReadDirPacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_READ_DIRECTORY_PACKET_BODY_FIELDS,
			&[Valuable::as_value(&self.file_descriptor)],
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
	pub async fn can_handle_read_directory() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
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
		// Create a file to be returned.
		_ = tokio::fs::File::create(join_many(&base_dir, ["cafe.bat"]))
			.await
			.expect("Failed to create file to use!");

		let dfd = fs
			.open_folder(&base_dir)
			.await
			.expect("Failed to open existing directory!");
		let request = SataReadDirPacketBody {
			file_descriptor: dfd,
		};

		// First request should return file information, and path name.
		let actual_file_response = request
			.handle(&mocked_header, &fs)
			.await
			.expect("Failed to get file information back from directory that was opened!");
		assert_eq!(
			&actual_file_response[0x20..0x48],
			&[
				0x00, 0x00, 0x00, 0x00, // RC
				0x2C, 0x00, 0x00, 0x00, // We should've gotten a file.
				0x00, 0x00, 0x06, 0x66, // Permissions
				0x00, 0x00, 0x00, 0x01, // Hardcoded 1
				0x00, 0x00, 0x00, 0x01, // Hardcoded 1
				0x00, 0x00, 0x00, 0x00, // File size.
				0x00, 0x00, 0x00, 0x00, // Hardcoded 0.
				0x00, 0x00, 0x00, 0xE8, // Hardcoded E8.
				0xDA, 0x6F, 0xF0, 0x00, // Hardcoded da6ff000.
				0x00, 0x00, 0x00, 0x00, // Hardcoded 0.
			],
		);
		assert_ne!(
			&actual_file_response[0x48..0x54],
			&[0_u8, 0, 0, 0, 0, 0, 0, 0],
		);
		assert_ne!(
			&actual_file_response[0x54..0x5C],
			&[0_u8, 0, 0, 0, 0, 0, 0, 0],
		);
		assert_eq!(
			&actual_file_response[0x5C..0x78],
			&[
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			],
		);
		assert_eq!(
			&actual_file_response[0x78..],
			&[
				0x63, 0x61, 0x66, 0x65, 0x2e, 0x62, 0x61, 0x74, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00, 0x00, 0x00,
			]
		);

		// Second one is an empty response.
		let new_response = request
			.handle(&mocked_header, &fs)
			.await
			.expect("Failed to get file information back from directory that was opened!");
		assert_eq!(&new_response[0x20..0x24], &[0xFF, 0xF0, 0xFF, 0xFC]);
		fs.close_folder(dfd).await;
	}
}
