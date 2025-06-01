//! Handling opening a folder packet.
//!
//! Since you can't really open a folder since it's not like a file, opening a
//! folder is more like an iterator over file names/folder names in the
//! directory, non recursively.

use crate::{
	fsemul::{
		host_filesystem::ResolvedLocation,
		pcfs::sata::{
			proto::{
				SataFileDescriptorResult, SataOpenFolderPacketBody, SataRequest, SataResponse,
			},
			server::PCFSServerState,
		},
	},
	net::{
		additions::StreamID,
		server::requestable::{Body, State},
	},
};
use tracing::debug;

/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;
/// An error code to send when a path does not exist.
///
/// This is also used in some places that are a bit of a stretch like for
/// network shares on disk space. The path doesn't exist on a disk, so this
/// error code is used, even if it's not quite exact.
const PATH_NOT_EXIST_ERROR: u32 = 0xFFF0_FFE9;

/// Handle opening a folder upon request.
pub async fn handle_open_folder(
	stream: StreamID,
	State(state): State<PCFSServerState>,
	Body(request): Body<SataRequest<SataOpenFolderPacketBody>>,
) -> SataResponse<SataFileDescriptorResult> {
	let packet = request.body();
	let request_header = request.header().clone();

	let Ok(final_location) = state.host_filesystem().resolve_path(packet.path()) else {
		debug!(
			packet.path = packet.path(),
			packet.typ = "PCFSSrvOpenFolder",
			"Failed to resolve path!",
		);

		return SataResponse::new(
			state.pid(),
			request_header,
			SataFileDescriptorResult::error(PATH_NOT_EXIST_ERROR),
		);
	};
	let ResolvedLocation::Filesystem(fs_location) = final_location else {
		todo!("network shares not yet implemented!")
	};

	let Ok(fd) = state
		.host_filesystem()
		.open_folder(fs_location.resolved_path(), Some(stream.to_raw()))
		.await
	else {
		debug!(
			packet.path = packet.path(),
			packet.typ = "PCFSSrvOpenFolder",
			"Failed to open folder!",
		);

		return SataResponse::new(
			state.pid(),
			request_header,
			SataFileDescriptorResult::error(FS_ERROR),
		);
	};

	debug!(
		result.fd = fd,
		packet.path = packet.path(),
		packet.typ = "PCFSSrvOpenFolder",
		"Successfully opened directory!",
	);

	SataResponse::new(
		state.pid(),
		request_header,
		SataFileDescriptorResult::success(fd),
	)
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
	pub async fn simple_open_folder_request() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
		let request = SataOpenFolderPacketBody::new("/%SLC_EMU_DIR/to-query/".to_owned())
			.expect("Failed to create open folder packet!");
		let mocked_header = SataPacketHeader::new(0);
		let mocked_ci = SataCommandInfo::new((0, 0), (0, 0), 0);

		let base_dir = join_many(tempdir.path(), ["data", "slc", "to-query"]);
		tokio::fs::create_dir(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");

		let mut response: Bytes = handle_open_folder(
			StreamID::from_existing(1),
			State(PCFSServerState::new(true, fs.clone(), 0)),
			Body(SataRequest::new(mocked_header, mocked_ci, request)),
		)
		.await
		.try_into()
		.expect("Failed to serialize open file response!");
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
		fs.close_folder(
			i32::from_be_bytes([response[4], response[5], response[6], response[7]]),
			Some(1),
		)
		.await;
	}
}
