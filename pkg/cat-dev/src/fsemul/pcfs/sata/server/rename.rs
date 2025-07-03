//! Handle rename packets which can renamefiles or folders.

use crate::{
	fsemul::{
		host_filesystem::ResolvedLocation,
		pcfs::sata::{
			proto::{SataRenamePacketBody, SataRequest, SataResponse, SataResultCode},
			server::PCFSServerState,
		},
	},
	net::server::requestable::{Body, State},
};
use tracing::{debug, error};

/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;
/// An error code to send when a path does not exist.
///
/// This is also used in some places that are a bit of a stretch like for
/// network shares on disk space. The path doesn't exist on a disk, so this
/// error code is used, even if it's not quite exact.
const PATH_NOT_EXIST_ERROR: u32 = 0xFFF0_FFE9;

/// Handle renaming a file, or directory upon request.
pub async fn handle_rename(
	State(state): State<PCFSServerState>,
	Body(request): Body<SataRequest<SataRenamePacketBody>>,
) -> SataResponse<SataResultCode> {
	let request_header = request.header().clone();
	let packet = request.body();

	let Ok(final_source_location) = state.host_filesystem().resolve_path(packet.source_path())
	else {
		debug!(
			packet.path = packet.source_path(),
			packet.typ = "PCFSSrvRename",
			"Failed to resolve path!",
		);

		return SataResponse::new(
			state.pid(),
			request_header,
			SataResultCode::error(PATH_NOT_EXIST_ERROR),
		);
	};
	let ResolvedLocation::Filesystem(fs_source_location) = final_source_location else {
		todo!("network shares not yet implemented!")
	};

	if !fs_source_location.resolved_path().exists() {
		return SataResponse::new(
			state.pid(),
			request_header,
			SataResultCode::error(PATH_NOT_EXIST_ERROR),
		);
	}

	let Ok(final_dest_location) = state.host_filesystem().resolve_path(packet.dest_path()) else {
		debug!(
			packet.path = packet.dest_path(),
			packet.typ = "PCFSSrvRename",
			"Failed to resolve path!",
		);

		return SataResponse::new(
			state.pid(),
			request_header,
			SataResultCode::error(PATH_NOT_EXIST_ERROR),
		);
	};
	let ResolvedLocation::Filesystem(fs_dest_location) = final_dest_location else {
		todo!("network shares not yet implemented!")
	};

	let result = if request.command_info().user() == (0x1000_00F5, 0x1000_00FF) {
		state
			.host_filesystem
			.copy(
				fs_source_location.resolved_path(),
				fs_dest_location.resolved_path(),
			)
			.await
	} else {
		state
			.host_filesystem
			.rename(
				fs_source_location.resolved_path(),
				fs_dest_location.resolved_path(),
			)
			.await
	};

	if let Err(cause) = result {
		error!(
			?cause,
			packet.source_path = packet.source_path(),
			packet.dest_path = packet.dest_path(),
			packet.typ = "PCFSSrvRename",
			"Failed to rename file or folder!",
		);

		SataResponse::new(state.pid(), request_header, SataResultCode::error(FS_ERROR))
	} else {
		SataResponse::new(
			state.pid(),
			request_header,
			SataResultCode::error(0xFFF0_FFEA),
		)
	}
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
	pub async fn test_rename() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;

		let base_dir = join_many(tempdir.path(), ["a", "b", "c"]);
		tokio::fs::create_dir_all(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		let file_path = join_many(&base_dir, ["file.txt"]);
		tokio::fs::write(&file_path, vec![0; 1307])
			.await
			.expect("Failed to write test file!");

		let inner_path = join_many(tempdir.path(), ["a", "b", "c", "d", "e"]);
		tokio::fs::create_dir_all(&inner_path)
			.await
			.expect("Failed to create temporary directory for test!");

		let directory_to_symlink = join_many(tempdir.path(), ["data", "slc"]);
		let dir_path_to_symlink = join_many(tempdir.path(), ["a", "b", "c", "d", "e", "f"]);

		let file_path_to_symlink = join_many(
			tempdir.path(),
			["a", "b", "c", "d", "e", "symlinked-file.txt"],
		);

		#[cfg(unix)]
		{
			use std::os::unix::fs::symlink;

			symlink(&directory_to_symlink, &dir_path_to_symlink)
				.expect("Failed to symlink directory!");
			symlink(&file_path, &file_path_to_symlink).expect("Failed to symlink file!");
		}

		#[cfg(target_os = "windows")]
		{
			use std::os::windows::fs::{symlink_dir, symlink_file};

			symlink_dir(&directory_to_symlink, &dir_path_to_symlink)
				.expect("Failed to symlink directory!");
			symlink_file(&file_path, &file_path_to_symlink).expect("Failed to symlink file!");
		}

		let renamed_dir = join_many(tempdir.path(), ["a", "b", "c.rm"]);
		let request = SataRenamePacketBody::new(
			base_dir
				.to_str()
				.expect("Test paths must be UTF-8")
				.to_owned(),
			renamed_dir
				.to_str()
				.expect("Test paths must be utf-8")
				.to_owned(),
		)
		.expect("Failed to create sata rename packet body!");
		let mocked_header = SataPacketHeader::new(0);
		let mocked_ci = SataCommandInfo::new((0, 0), (0, 0), 0);

		let _bytes: Bytes = handle_rename(
			State(PCFSServerState::new(true, fs, 0)),
			Body(SataRequest::new(mocked_header, mocked_ci, request)),
		)
		.await
		.try_into()
		.expect("Failed to serialize real removal response!");

		assert!(
			!base_dir.exists(),
			"Base directory still exists post 'removal'",
		);
		assert!(renamed_dir.exists(), "Renamed directory doesn't exist?");
	}
}
