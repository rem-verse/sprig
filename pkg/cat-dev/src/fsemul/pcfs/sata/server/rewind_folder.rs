//! Handle rewinding a directory or 'folders' iterator when a folder is
//! actively open in PCFS.

use crate::{
	fsemul::pcfs::sata::{
		proto::{SataRequest, SataResponse, SataResultCode, SataRewindFolderPacketBody},
		server::PCFSServerState,
	},
	net::server::requestable::{Body, State},
};
use tracing::debug;

/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;

/// Actually process by rewinding an open directory iterator.
pub async fn handle_rewind_folder(
	State(state): State<PCFSServerState>,
	Body(request): Body<SataRequest<SataRewindFolderPacketBody>>,
) -> SataResponse<SataResultCode> {
	let request_header = request.header().clone();
	let packet = request.body();

	if state
		.host_filesystem()
		.reverse_folder(packet.file_descriptor())
		.await
		.is_err()
	{
		debug!(
			packet.fd = packet.file_descriptor(),
			packet.typ = "PCFSSrvRewindDirectory",
			"Failed to rewind directory!",
		);

		return SataResponse::new(state.pid(), request_header, SataResultCode::error(FS_ERROR));
	}

	SataResponse::new(state.pid(), request_header, SataResultCode::success())
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
	pub async fn can_handle_rewind_directory() {
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
			.open_folder(&base_dir)
			.await
			.expect("Failed to open existing directory!");
		let request = SataRewindFolderPacketBody::new(dfd);

		// First request should return file information, and path name.
		let actual_response: Bytes = handle_rewind_folder(
			State(PCFSServerState::new(true, fs.clone(), 0)),
			Body(SataRequest::new(mocked_header, mocked_ci, request)),
		)
		.await
		.try_into()
		.expect("Failed to serialize rewind folder response!");
		assert_eq!(&actual_response[0x20..], &[0x00, 0x00, 0x00, 0x00]);
		fs.close_folder(dfd).await;
	}
}
