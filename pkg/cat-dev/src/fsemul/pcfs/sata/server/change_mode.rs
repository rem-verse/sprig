//! Handle "change mode" packets, which basically just set read only flags.

use crate::{
	errors::FSError,
	fsemul::{
		host_filesystem::ResolvedLocation,
		pcfs::sata::{
			proto::{SataChangeModePacketBody, SataRequest, SataResponse, SataResultCode},
			server::PCFSServerState,
		},
	},
	net::server::requestable::{Body, State},
};
use std::fs::set_permissions;
use tracing::debug;

/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;
/// An error code to send when a path does not exist.
///
/// This is also used in some places that are a bit of a stretch like for
/// network shares on disk space. The path doesn't exist on a disk, so this
/// error code is used, even if it's not quite exact.
const PATH_NOT_EXIST_ERROR: u32 = 0xFFF0_FFE9;

/// Handle a Change Mode Request.
///
/// This handles setting read only mode one or off.
pub async fn handle_change_mode(
	State(state): State<PCFSServerState>,
	Body(request): Body<SataRequest<SataChangeModePacketBody>>,
) -> SataResponse<SataResultCode> {
	let request_header = request.header().clone();
	let packet = request.body();
	let Ok(final_location) = state.host_filesystem().resolve_path(packet.path()) else {
		debug!(
			packet.path = packet.path(),
			packet.typ = "PCFSSrvChangeMode",
			"Failed to resolve path!",
		);

		return SataResponse::new(
			state.pid(),
			request_header,
			SataResultCode::error(PATH_NOT_EXIST_ERROR),
		);
	};
	let ResolvedLocation::Filesystem(fs_location) = final_location else {
		todo!("network shares not yet implemented!")
	};
	// Path doesn't exist.
	if !fs_location.canonicalized_is_exact() {
		debug!(
			packet.path = packet.path(),
			packet.typ = "PCFSSrvChangeMode",
			"Cannot change mode of path that does not exist!",
		);

		return SataResponse::new(
			state.pid(),
			request_header,
			SataResultCode::error(PATH_NOT_EXIST_ERROR),
		);
	}

	let Ok(metadata) = fs_location.closest_resolved_path().metadata() else {
		debug!(
			packet.path = packet.path(),
			packet.typ = "PCFSSrvChangeMode",
			"Failed to get path metadata!",
		);

		return SataResponse::new(state.pid(), request_header, SataResultCode::error(FS_ERROR));
	};

	// If we're changing to write, we need to do a path check.
	let mut perms = metadata.permissions();
	if packet.will_set_write_mode()
		&& !state
			.host_filesystem()
			.path_allows_writes(fs_location.closest_resolved_path())
	{
		debug!(
			packet.path = packet.path(),
			packet.typ = "PCFSSrvChangeMode",
			"Path cannot become writable!",
		);

		return SataResponse::new(state.pid(), request_header, SataResultCode::error(FS_ERROR));
	}
	perms.set_readonly(!packet.will_set_write_mode());
	// Don't set folders as read-only.
	//
	// Windows 7 (where Cafe-SDK targeted), allows you to create files
	// within a "read only" directory. The SDK depends on this "buggy"
	// behavior. It will set read only attributes on a directory, and then
	// attempt to create files in that directory anyway.
	//
	// Thanks Windows :)
	let result = if fs_location.closest_resolved_path().is_dir() {
		if packet.will_set_write_mode() {
			state
				.host_filesystem()
				.ensure_folder_not_read_only(fs_location.closest_resolved_path())
				.await;
			Ok(())
		} else {
			state
				.host_filesystem()
				.mark_folder_read_only(fs_location.closest_resolved_path().clone())
				.await
		}
	} else {
		set_permissions(fs_location.closest_resolved_path(), perms).map_err(FSError::IO)
	};

	if result.is_err() {
		debug!(
			cause = ?result,
			packet.path = packet.path(),
			packet.typ = "PCFSSrvChangeMode",
			"Failed to change read-only attribute!",
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
	pub async fn change_mode_request() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
		let request =
			SataChangeModePacketBody::new("/%SLC_EMU_DIR/to-query/file.txt".to_owned(), false)
				.expect("Failed to create change mode packet!");
		let mocked_header = SataPacketHeader::new(0);
		let mocked_ci = SataCommandInfo::new((0, 0), (0, 0), 0);

		// Okay let's create some files and some sizes in there.
		let base_dir = join_many(tempdir.path(), ["data", "slc", "to-query"]);
		tokio::fs::create_dir(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		tokio::fs::write(join_many(&base_dir, ["file.txt"]), vec![0; 1307])
			.await
			.expect("Failed to write test file!");

		let mut mode_resp: Bytes = handle_change_mode(
			State(PCFSServerState::new(true, fs, 0)),
			Body(SataRequest::new(mocked_header, mocked_ci, request)),
		)
		.await
		.try_into()
		.expect("Failed to serialize change mode response!");
		assert_eq!(mode_resp.len(), 4 + 0x20, "Packet is not correct size!");
		// Okay first chop off the header, we don't care.
		_ = mode_resp.split_to(0x20);
		assert_eq!(
			mode_resp,
			Bytes::from(vec![
				0x00, 0x00, 0x00, 0x00, // RC
			]),
		);
	}
}
