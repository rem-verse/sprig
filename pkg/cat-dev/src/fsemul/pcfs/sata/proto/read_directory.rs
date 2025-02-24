//! Definitions, and handlers for the `ReadDirectory` packet type.
//!
//! This will return the file information for the next file present within a
//! directory. This does not recurse.

use crate::errors::NetworkParseError;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

#[cfg(feature = "clients")]
use crate::fsemul::pcfs::{errors::PCFSApiError, sata::proto::get_info_by_query::PCFSSataFdInfo};
#[cfg(feature = "clients")]
use std::ffi::CStr;

#[cfg(feature = "servers")]
use crate::{
	errors::CatBridgeError,
	fsemul::{
		host_filesystem::HostFilesystem,
		pcfs::sata::proto::{
			SataGetInfoByQueryPacketBody, SataPacketHeader, construct_sata_response,
		},
	},
};
#[cfg(feature = "servers")]
use std::path::PathBuf;
#[cfg(feature = "servers")]
use tracing::debug;

#[cfg(feature = "servers")]
/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;
#[cfg(any(feature = "clients", feature = "servers"))]
/// No more items in this directory! sorry!
const NO_MORE_ITEMS: u32 = 0xFFF0_FFFC;

/// A packet to get information about another file within a directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataReadDirPacketBody {
	file_descriptor: i32,
}

impl SataReadDirPacketBody {
	/// Create a new read directory packet body.
	#[must_use]
	pub const fn new(file_descriptor: i32) -> Self {
		Self { file_descriptor }
	}

	#[must_use]
	pub const fn file_descriptor(&self) -> i32 {
		self.file_descriptor
	}

	pub const fn set_file_descriptor(&mut self) -> i32 {
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
		let Ok(info) = SataGetInfoByQueryPacketBody::info_for_path(host_filesystem, &item).await
		else {
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

impl From<&SataReadDirPacketBody> for Bytes {
	fn from(value: &SataReadDirPacketBody) -> Self {
		let mut buff = BytesMut::with_capacity(4);
		buff.put_i32(value.file_descriptor);
		buff.freeze()
	}
}

impl From<SataReadDirPacketBody> for Bytes {
	fn from(value: SataReadDirPacketBody) -> Self {
		Self::from(&value)
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

#[cfg(feature = "clients")]
#[derive(Clone, Debug, PartialEq, Eq, Valuable)]
pub struct DirectoryItemResponse {
	/// The underlying return code for this directory item.
	return_code: u32,
	/// The next bit of file information, if there are any items left in the
	/// directory.
	next_file_info: Option<(PCFSSataFdInfo, String)>,
}

#[cfg(feature = "clients")]
impl DirectoryItemResponse {
	/// Create a Directory Item.
	///
	/// This assumes there was another item in the directory and we want to
	/// return it.
	///
	/// ## Errors
	///
	/// If the path is longer than 255 bytes.
	pub fn new_next(info: PCFSSataFdInfo, path: String) -> Result<Self, PCFSApiError> {
		if path.len() > 255 {
			return Err(PCFSApiError::PathTooLong(path));
		}

		Ok(Self {
			return_code: 0,
			next_file_info: Some((info, path)),
		})
	}

	/// Return that there's nothing left in the directory.
	#[must_use]
	pub const fn new_nothing_left() -> Self {
		Self {
			return_code: NO_MORE_ITEMS,
			next_file_info: None,
		}
	}

	/// Create a new directory item response given an error code.
	#[must_use]
	pub const fn new_error_code(rc: u32) -> Self {
		Self {
			return_code: rc,
			next_file_info: None,
		}
	}

	/// Get the underlying return code for this directory item.
	#[must_use]
	pub const fn return_code(&self) -> u32 {
		self.return_code
	}

	/// Return if this directory item was 'successfully' fetched.
	///
	/// This is really only useful when performing manual construction of the
	/// read directory packet. As when deserializing we will always return a
	/// proper error code if it's not successful.
	#[must_use]
	pub fn is_successful(&self) -> bool {
		[NO_MORE_ITEMS, 0].contains(&self.return_code)
	}

	/// Get the file that was returned as being next in the directory.
	#[must_use]
	pub const fn file_info(&self) -> Option<&(PCFSSataFdInfo, String)> {
		self.next_file_info.as_ref()
	}

	/// Consume the underlying packet, and just get the next file info if there
	/// is any.
	#[must_use]
	pub fn take_file_info(self) -> Option<(PCFSSataFdInfo, String)> {
		self.next_file_info
	}
}

#[cfg(feature = "clients")]
impl From<&DirectoryItemResponse> for Bytes {
	fn from(value: &DirectoryItemResponse) -> Self {
		if let Some(nfi) = value.file_info() {
			let mut buff = BytesMut::with_capacity(0x158);
			buff.put_u32(value.return_code());
			buff.extend(Bytes::from(&nfi.0));
			let path_bytes = nfi.1.as_bytes();
			buff.extend(Bytes::from(Vec::from(path_bytes)));
			buff.extend(BytesMut::zeroed(256 - path_bytes.len()));
			buff.freeze()
		} else {
			let mut bytes = BytesMut::with_capacity(4);
			bytes.put_u32(value.return_code());
			bytes.freeze()
		}
	}
}

#[cfg(feature = "clients")]
impl From<DirectoryItemResponse> for Bytes {
	fn from(value: DirectoryItemResponse) -> Self {
		Self::from(&value)
	}
}

#[cfg(feature = "clients")]
impl TryFrom<Bytes> for DirectoryItemResponse {
	type Error = NetworkParseError;

	fn try_from(mut value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 4 {
			return Err(NetworkParseError::NotEnoughData(
				"DirectoryItemResponse",
				4,
				value.len(),
				value,
			));
		}

		// Okay we've at least got an rc....
		let rc = value.get_u32();
		if rc != 0 && rc != NO_MORE_ITEMS {
			return Err(NetworkParseError::ErrorCode(rc));
		}
		if rc == NO_MORE_ITEMS {
			return Ok(DirectoryItemResponse::new_nothing_left());
		}

		// rc is now guaranteed to be 0... We should expect a full packet.
		if value.len() < 0x154 {
			return Err(NetworkParseError::NotEnoughData(
				"DirectoryItemResponse",
				0x154,
				value.len(),
				value,
			));
		}
		if value.len() > 0x154 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"DirectoryItemResponse",
				value.slice(0x154..),
			));
		}

		let fd_info = PCFSSataFdInfo::try_from(value.slice(..84))?;
		let path_bytes = value.slice(84..);
		let path =
			CStr::from_bytes_until_nul(&path_bytes).map_err(NetworkParseError::BadCString)?;

		Ok(Self {
			return_code: rc,
			next_file_info: Some((fd_info, path.to_str()?.to_owned())),
		})
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
