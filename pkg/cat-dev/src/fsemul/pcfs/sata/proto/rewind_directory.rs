//! Definitions, and handlers for the `RewindDirectory` packet type.
//!
//! This will return the directory iterator to the very beginning of the
//! directory regardless of where it is.

use crate::errors::NetworkParseError;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

#[cfg(feature = "servers")]
use crate::{
	errors::CatBridgeError,
	fsemul::{
		host_filesystem::HostFilesystem,
		pcfs::sata::proto::{SataPacketHeader, construct_sata_response},
	},
};
#[cfg(feature = "servers")]
use tracing::debug;

#[cfg(feature = "servers")]
/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;

/// A packet to rewind to the beginning of a directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataRewindDirPacketBody {
	file_descriptor: i32,
}

impl SataRewindDirPacketBody {
	/// Create a new rewind directory packet.
	#[must_use]
	pub const fn new(file_descriptor: i32) -> Self {
		Self { file_descriptor }
	}

	#[must_use]
	pub const fn file_descriptor(&self) -> i32 {
		self.file_descriptor
	}

	pub const fn set_file_descriptor(&mut self, new_fd: i32) {
		self.file_descriptor = new_fd;
	}

	/// Actually process by rewinding an open directory iterator.
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
		if host_filesystem
			.reverse_directory(self.file_descriptor)
			.await
			.is_err()
		{
			debug!(
				packet.fd = self.file_descriptor,
				packet.typ = "PCFSSrvRewindDirectory",
				"Failed to rewind directory!",
			);

			return Self::construct_error_repsonse(request_header, FS_ERROR);
		}

		Ok(construct_sata_response(
			request_header,
			0,
			BytesMut::zeroed(4).freeze(),
		)?)
	}

	#[cfg(feature = "servers")]
	fn construct_error_repsonse(
		request_header: &SataPacketHeader,
		error_code: u32,
	) -> Result<Bytes, CatBridgeError> {
		let mut buff = BytesMut::with_capacity(4);
		buff.put_u32(error_code);
		Ok(construct_sata_response(request_header, 0, buff.freeze())?)
	}
}

impl From<&SataRewindDirPacketBody> for Bytes {
	fn from(value: &SataRewindDirPacketBody) -> Self {
		let mut buff = BytesMut::with_capacity(4);
		buff.put_i32(value.file_descriptor);
		buff.freeze()
	}
}

impl From<SataRewindDirPacketBody> for Bytes {
	fn from(value: SataRewindDirPacketBody) -> Self {
		Self::from(&value)
	}
}

impl TryFrom<Bytes> for SataRewindDirPacketBody {
	type Error = NetworkParseError;

	fn try_from(mut value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x4 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataRewindDir",
				"Body",
				0x4,
				value.len(),
				value,
			));
		}
		if value.len() > 0x4 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataRewindDir",
				value.slice(0x4..),
			));
		}

		let fd = value.get_i32();

		Ok(Self {
			file_descriptor: fd,
		})
	}
}

const SATA_REWIND_DIRECTORY_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[NamedField::new("fd")];

impl Structable for SataRewindDirPacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataRewindDirPacketBody",
			Fields::Named(SATA_REWIND_DIRECTORY_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataRewindDirPacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_REWIND_DIRECTORY_PACKET_BODY_FIELDS,
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
	pub async fn can_handle_rewind_directory() {
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
		let request = SataRewindDirPacketBody {
			file_descriptor: dfd,
		};

		// First request should return file information, and path name.
		let actual_response = request
			.handle(&mocked_header, &fs)
			.await
			.expect("Failed to get file information back from directory that was opened!");
		assert_eq!(&actual_response[0x20..], &[0x00, 0x00, 0x00, 0x00]);
		fs.close_folder(dfd).await;
	}
}
