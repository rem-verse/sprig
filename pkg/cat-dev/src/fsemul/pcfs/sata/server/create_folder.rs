//! Create a new folder over PCFS.

use crate::{
	fsemul::{
		host_filesystem::ResolvedLocation,
		pcfs::sata::{
			proto::{SataCreateFolderPacketBody, SataRequest, SataResponse, SataResultCode},
			server::PCFSServerState,
		},
	},
	net::server::requestable::{Body, State},
};
use tokio::fs::create_dir_all;
use tracing::{debug, error};

/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;

/// Handle creating a directory upon request.
pub async fn handle_create_folder(
	State(state): State<PCFSServerState>,
	Body(request): Body<SataRequest<SataCreateFolderPacketBody>>,
) -> SataResponse<SataResultCode> {
	let packet = request.body();
	let request_header = request.header().clone();

	let Ok(final_location) = state.host_filesystem().resolve_path(packet.path()) else {
		debug!(
			packet.path = packet.path(),
			packet.typ = "PCFSSrvCreateDirectory",
			"Failed to resolve path!",
		);

		return SataResponse::new(state.pid(), request_header, SataResultCode::error(FS_ERROR));
	};
	let ResolvedLocation::Filesystem(fs_location) = final_location else {
		todo!("network shares not yet implemented!")
	};

	// Do path check for writes.
	if packet.will_set_write_mode()
		&& !state
			.host_filesystem()
			.path_allows_writes(fs_location.closest_resolved_path())
	{
		debug!(
			packet.path = packet.path(),
			packet.typ = "PCFSSrvCreateDirectory",
			"Cannot create directory in read-only path!",
		);

		return SataResponse::new(state.pid(), request_header, SataResultCode::error(FS_ERROR));
	}

	if !fs_location.resolved_path().exists()
		&& let Err(cause) = create_dir_all(fs_location.resolved_path()).await
	{
		error!(
		  ?cause,
		  path = %fs_location.resolved_path().display(),
		  "Failed to create directory for PCFS.",
		);

		return SataResponse::new(state.pid(), request_header, SataResultCode::error(FS_ERROR));
	}

	// Don't set folders as read-only.
	//
	// Windows 7 (where Cafe-SDK targeted), allows you to create files
	// within a "read only" directory. The SDK depends on this "buggy"
	// behavior. It will set read only attributes on a directory, and then
	// attempt to create files in that directory anyway.
	//
	// Thanks Windows :)
	if packet.will_set_write_mode() {
		state
			.host_filesystem()
			.ensure_folder_not_read_only(fs_location.resolved_path())
			.await;
	} else {
		state
			.host_filesystem()
			.mark_folder_read_only(fs_location.resolved_path().clone())
			.await;
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
	pub async fn test_simple_create_directory() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;

		let base_dir = join_many(tempdir.path(), ["a", "b", "c"]);

		let request = SataCreateFolderPacketBody::new(
			base_dir
				.to_str()
				.expect("Test paths must be UTF-8")
				.to_owned(),
			true,
		)
		.expect("Failed to create sata folder packet body!");
		let mocked_header = SataPacketHeader::new(0);
		let mocked_info = SataCommandInfo::new((0, 0), (0, 0), 0);

		assert!(!base_dir.exists());
		let _response: Bytes = handle_create_folder(
			State(PCFSServerState::new(true, fs, 0)),
			Body(SataRequest::new(mocked_header, mocked_info, request)),
		)
		.await
		.try_into()
		.expect("Failed to serialize close folder response!");
		assert!(base_dir.exists());
	}
}
