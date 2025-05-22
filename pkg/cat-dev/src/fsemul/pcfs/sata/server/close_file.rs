//! Handle a client requesting us to close an open file.

use crate::{
	fsemul::pcfs::sata::{
		proto::{SataCloseFilePacketBody, SataPacketHeader, SataResponse, SataResultCode},
		server::PCFSServerState,
	},
	net::server::requestable::{Body, State},
};

/// Handle closing a file that was previously open.
pub async fn handle_close_file(
	request_header: SataPacketHeader,
	State(state): State<PCFSServerState>,
	// Validate that the body is actually a change owner packet.
	Body(packet): Body<SataCloseFilePacketBody>,
) -> SataResponse<SataResultCode> {
	state
		.host_filesystem()
		.close_file(packet.file_descriptor())
		.await;
	SataResponse::new(state.pid(), request_header, SataResultCode::success())
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use crate::fsemul::host_filesystem::test_helpers::{
		create_temporary_host_filesystem, join_many,
	};
	use bytes::Bytes;
	use tokio::fs::OpenOptions;

	#[tokio::test]
	pub async fn simple_close_file_request() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;

		let base_dir = join_many(tempdir.path(), ["data", "slc", "to-query"]);
		tokio::fs::create_dir(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		tokio::fs::write(join_many(&base_dir, ["file.txt"]), vec![0; 2])
			.await
			.expect("Failed to write test file!");
		let mocked_header = SataPacketHeader::new(0);
		let mut open_options = OpenOptions::new();
		open_options.read(true).create(false).write(false);
		let fd = fs
			.open_file(open_options, &join_many(&base_dir, ["file.txt"]))
			.await
			.expect("Failed to open file!");

		let close_request = SataCloseFilePacketBody::new(fd);
		let response: Bytes = handle_close_file(
			mocked_header.clone(),
			State(PCFSServerState::new(true, fs, 0)),
			Body(close_request),
		)
		.await
		.try_into()
		.expect("Failed to serialize close file response!");
		assert_eq!(&response[0x20..], &[0; 4]);
	}
}
