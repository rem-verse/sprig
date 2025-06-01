//! Handle a close folder request.
//!
//! This assumes someone else has opened a directory on the same connection.

use crate::{
	fsemul::pcfs::sata::{
		proto::{SataCloseFolderPacketBody, SataRequest, SataResponse, SataResultCode},
		server::PCFSServerState,
	},
	net::{
		additions::StreamID,
		server::requestable::{Body, State},
	},
};

/// Handle closing a folder that was previously open.
pub async fn handle_close_folder(
	stream: StreamID,
	State(state): State<PCFSServerState>,
	Body(request): Body<SataRequest<SataCloseFolderPacketBody>>,
) -> SataResponse<SataResultCode> {
	let packet = request.body();
	state
		.host_filesystem()
		.close_folder(packet.file_descriptor(), Some(stream.to_raw()))
		.await;

	SataResponse::new(
		state.pid(),
		request.header().clone(),
		SataResultCode::success(),
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
	use tokio::fs::OpenOptions;

	#[tokio::test]
	pub async fn simple_close_folder_request() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;

		let base_dir = join_many(tempdir.path(), ["data", "slc", "to-query"]);
		tokio::fs::create_dir(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		tokio::fs::write(join_many(&base_dir, ["file.txt"]), vec![0; 2])
			.await
			.expect("Failed to write test file!");
		let mocked_header = SataPacketHeader::new(0);
		let mocked_ci = SataCommandInfo::new((0, 0), (0, 0), 0);

		let mut open_options = OpenOptions::new();
		open_options.read(true).create(false).write(false);
		let fd = fs
			.open_file(open_options, &join_many(&base_dir, ["file.txt"]), Some(1))
			.await
			.expect("Failed to open file!");
		let close_request = SataCloseFolderPacketBody::new(fd);

		let response: Bytes = handle_close_folder(
			StreamID::from_existing(1),
			State(PCFSServerState::new(true, fs, 0)),
			Body(SataRequest::new(mocked_header, mocked_ci, close_request)),
		)
		.await
		.try_into()
		.expect("Failed to serialize close folder response!");
		assert_eq!(&response[0x20..], &[0; 4]);
	}
}
