//! Handle iterating over a directory, getting a file that is in a directory.

use crate::{
	errors::CatBridgeError,
	fsemul::pcfs::sata::{
		proto::{
			DirectoryItemResponse, SataFDInfo, SataReadFolderPacketBody, SataRequest, SataResponse,
		},
		server::PCFSServerState,
	},
	net::{
		additions::StreamID,
		server::requestable::{Body, State},
	},
};
use std::path::PathBuf;
use tracing::debug;

/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;

/// Create a packet to then get the file information in the next file in a
/// folder.
///
/// ## Errors
///
/// If the path is too long to encode into a packet.
pub async fn handle_read_folder(
	stream: StreamID,
	State(state): State<PCFSServerState>,
	Body(request): Body<SataRequest<SataReadFolderPacketBody>>,
) -> Result<SataResponse<DirectoryItemResponse>, CatBridgeError> {
	let packet = request.body();
	let request_header = request.header().clone();

	let fd = packet.file_descriptor();
	let Ok(optional_next_item) = state
		.host_filesystem()
		.next_in_folder(fd, Some(stream.to_raw()))
		.await
	else {
		debug!(
			packet.fd = fd,
			packet.typ = "PCFSSrvReadDirectory",
			"Failed to query for next item in folder!",
		);

		return Ok(SataResponse::new(
			state.pid(),
			request_header,
			DirectoryItemResponse::new_error_code(FS_ERROR),
		));
	};
	let Some((item, components_to_remove)) = optional_next_item else {
		debug!(
			packet.fd = fd,
			packet.typ = "PCFSSrvReadDirectory",
			"No more items in directory!",
		);

		return Ok(SataResponse::new(
			state.pid(),
			request_header,
			DirectoryItemResponse::new_nothing_left(),
		));
	};

	let Ok(md) = item.metadata() else {
		debug!(
			packet.fd = fd,
			packet.typ = "PCFSSrvReadDirectory",
			"Failed to get information for next file in folder!",
		);

		return Ok(SataResponse::new(
			state.pid(),
			request_header,
			DirectoryItemResponse::new_error_code(FS_ERROR),
		));
	};
	let info = SataFDInfo::get_info(state.host_filesystem(), &md, &item).await;

	let utf8 = item
		.components()
		.skip(components_to_remove)
		.collect::<PathBuf>()
		.to_string_lossy()
		.to_string();
	if utf8.len() > 255 {
		debug!(
			packet.fd = fd,
			packet.typ = "PCFSSrvReadDirectory",
			"UTF-8 path is too long, cant serve!",
		);

		return Ok(SataResponse::new(
			state.pid(),
			request_header,
			DirectoryItemResponse::new_error_code(FS_ERROR),
		));
	}

	Ok(SataResponse::new(
		state.pid(),
		request_header,
		DirectoryItemResponse::new_next(info, utf8)?,
	))
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use crate::fsemul::{
		host_filesystem::test_helpers::{create_temporary_host_filesystem, join_many},
		pcfs::sata::proto::{SataCommandInfo, SataPacketHeader},
	};
	use bytes::Bytes;

	#[tokio::test]
	pub async fn can_handle_read_directory() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
		let mocked_header = SataPacketHeader::new(0);
		let mocked_ci = SataCommandInfo::new((0, 0), (0, 0), 0);

		let base_dir = join_many(tempdir.path(), ["data", "slc", "to-query"]);
		tokio::fs::create_dir(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		// Create a file to be returned.
		_ = tokio::fs::File::create(join_many(&base_dir, ["cafe.bat"]))
			.await
			.expect("Failed to create file to use!");

		let dfd = fs
			.open_folder(&base_dir, Some(1))
			.await
			.expect("Failed to open existing directory!");
		let request = SataReadFolderPacketBody::new(dfd);

		// First request should return file information, and path name.
		let actual_file_response: Bytes = handle_read_folder(
			StreamID::from_existing(1),
			State(PCFSServerState::new(false, fs.clone(), 0)),
			Body(SataRequest::new(
				mocked_header.clone(),
				mocked_ci.clone(),
				request.clone(),
			)),
		)
		.await
		.expect("Failed to handle read folder request!")
		.try_into()
		.expect("Failed to serialize read folder response!");
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
		let new_response: Bytes = handle_read_folder(
			StreamID::from_existing(1),
			State(PCFSServerState::new(true, fs.clone(), 0)),
			Body(SataRequest::new(mocked_header, mocked_ci, request)),
		)
		.await
		.expect("Failed to handle read folder request!")
		.try_into()
		.expect("Failed to serialize read folder response!");
		assert_eq!(&new_response[0x20..0x24], &[0xFF, 0xF0, 0xFF, 0xFC]);
		fs.close_folder(dfd, Some(1)).await;
	}
}
