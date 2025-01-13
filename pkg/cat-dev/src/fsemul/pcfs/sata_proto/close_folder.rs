//! Definitions, and handlers for the `CloseFolder` packet type.
//!
//! Thise closes an already existing open folder given just a handle.

use crate::{
	errors::NetworkParseError,
	fsemul::{
		host_filesystem::HostFilesystem,
		pcfs::{
			errors::PCFSApiError,
			sata_proto::{construct_sata_response, SataPacketHeader},
		},
	},
};
use bytes::{Buf, Bytes, BytesMut};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// A packet to close a particular folder given a file descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataCloseFolderPacketBody {
	file_descriptor: i32,
}

impl SataCloseFolderPacketBody {
	#[must_use]
	pub const fn file_descriptor(&self) -> i32 {
		self.file_descriptor
	}

	/// Handle closing a folder that was previously open.
	///
	/// ## Errors
	///
	/// If we cannot construct a response.
	pub async fn handle(
		&self,
		request_header: &SataPacketHeader,
		host_filesystem: &HostFilesystem,
	) -> Result<Bytes, PCFSApiError> {
		host_filesystem.close_folder(self.file_descriptor).await;

		construct_sata_response(request_header, 0, BytesMut::zeroed(4).freeze())
	}
}

impl TryFrom<Bytes> for SataCloseFolderPacketBody {
	type Error = NetworkParseError;

	fn try_from(mut value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x4 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataCloseFolder",
				"Body",
				0x4,
				value.len(),
				value,
			));
		}
		if value.len() > 0x4 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataCloseFolder",
				value.slice(0x4..),
			));
		}

		let fd = value.get_i32();

		Ok(Self {
			file_descriptor: fd,
		})
	}
}

const SATA_CLOSE_FOLDER_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[NamedField::new("fd")];

impl Structable for SataCloseFolderPacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataCloseFolderPacketBody",
			Fields::Named(SATA_CLOSE_FOLDER_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataCloseFolderPacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_CLOSE_FOLDER_PACKET_BODY_FIELDS,
			&[Valuable::as_value(&self.file_descriptor)],
		));
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use crate::fsemul::host_filesystem::test_helpers::{
		create_temporary_host_filesystem, join_many,
	};
	use tokio::fs::OpenOptions;

	#[tokio::test]
	pub async fn simple_ffio_read_file_request() {
		let (tempdir, fs) = create_temporary_host_filesystem();

		let base_dir = join_many(tempdir.path(), ["data", "slc", "to-query"]);
		tokio::fs::create_dir(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		tokio::fs::write(join_many(&base_dir, ["file.txt"]), vec![0; 2])
			.await
			.expect("Failed to write test file!");
		let mocked_header = SataPacketHeader {
			packet_data_len: 0,
			packet_id: 0,
			flags: 0,
			version: 0,
			timestamp_on_host: 0,
			pid_on_host: 0,
		};

		let mut open_options = OpenOptions::new();
		open_options.read(true).create(false).write(false);
		let fd = fs
			.open_file(open_options, &join_many(&base_dir, ["file.txt"]))
			.await
			.expect("Failed to open file!");

		let close_request = SataCloseFolderPacketBody {
			file_descriptor: fd,
		};

		let response = close_request
			.handle(&mocked_header, &fs)
			.await
			.expect("Failed to handle read request!");
		assert_eq!(&response[0x20..], &[0; 4]);
	}
}
