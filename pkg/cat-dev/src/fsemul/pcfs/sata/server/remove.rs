//! Handle remove packets which can remove files or folders.

use crate::{
	fsemul::{
		host_filesystem::ResolvedLocation,
		pcfs::sata::{
			proto::{SataRemovePacketBody, SataRequest, SataResponse, SataResultCode},
			server::PCFSServerState,
		},
	},
	net::server::requestable::{Body, State},
};
use std::ffi::OsStr;
use tokio::fs::{remove_dir_all, remove_file};
use tracing::{debug, error};

/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;
/// An error code to send when a path does not exist.
///
/// This is also used in some places that are a bit of a stretch like for
/// network shares on disk space. The path doesn't exist on a disk, so this
/// error code is used, even if it's not quite exact.
const PATH_NOT_EXIST_ERROR: u32 = 0xFFF0_FFE9;

/// Handle removing a file, or directory upon request.
pub async fn handle_removal(
	State(state): State<PCFSServerState>,
	Body(request): Body<SataRequest<SataRemovePacketBody>>,
) -> SataResponse<SataResultCode> {
	let request_header = request.header().clone();
	let packet = request.body();

	let Ok(final_location) = state.host_filesystem().resolve_path(packet.path()) else {
		debug!(
			packet.path = packet.path(),
			packet.typ = "PCFSSrvRemoveFile",
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

	if !fs_location.resolved_path().exists() {
		return SataResponse::new(
			state.pid(),
			request_header,
			SataResultCode::error(PATH_NOT_EXIST_ERROR),
		);
	}

	if state.disable_real_removal() {
		let new_path = if fs_location.resolved_path().is_file() {
			let mut new_filename = fs_location
				.resolved_path()
				.file_name()
				.unwrap_or_default()
				.to_owned();
			new_filename.push(OsStr::new(".rm"));
			let mut new_path = fs_location.resolved_path().clone();
			new_path.pop();
			new_path.push(new_filename);
			new_path
		} else {
			let mut new_path = fs_location.resolved_path().clone();
			let mut dir_name = new_path
				.components()
				.next_back()
				.map(|c| c.as_os_str().to_os_string())
				.unwrap_or_default();
			dir_name.push(".rm");
			new_path.pop();
			new_path.push(dir_name);
			new_path
		};

		if let Err(cause) = state
			.host_filesystem()
			.rename(fs_location.resolved_path(), &new_path)
			.await
		{
			error!(
			  ?cause,
			  path = %fs_location.resolved_path().display(),
			  "Failed to remove/rename directory as requested by PCFS."
			);

			return SataResponse::new(state.pid(), request_header, SataResultCode::error(FS_ERROR));
		}
	} else if fs_location.resolved_path().is_file() {
		if let Err(cause) = remove_file(fs_location.resolved_path()).await {
			error!(
			  ?cause,
			  path = %fs_location.resolved_path().display(),
			  "Failed to remove file as requested by PCFS.",
			);

			return SataResponse::new(state.pid(), request_header, SataResultCode::error(FS_ERROR));
		}
	} else if fs_location.resolved_path().is_dir() {
		if let Err(cause) = remove_dir_all(fs_location.resolved_path()).await {
			error!(
			  ?cause,
			  path = %fs_location.resolved_path().display(),
			  "Failed to remove directory as requested by PCFS."
			);

			return SataResponse::new(state.pid(), request_header, SataResultCode::error(FS_ERROR));
		}
	} else {
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
	pub async fn test_real_removal() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;

		let base_dir = join_many(tempdir.path(), ["a", "b", "c"]);
		tokio::fs::create_dir_all(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		let file_path = join_many(&base_dir, ["file.txt"]);
		tokio::fs::write(&file_path, vec![0; 1307])
			.await
			.expect("Failed to write test file!");

		let request = SataRemovePacketBody::new(
			base_dir
				.to_str()
				.expect("Test paths must be UTF-8")
				.to_owned(),
		)
		.expect("Failed to create sata remove packet body!");
		let mocked_header = SataPacketHeader::new(0);
		let mocked_ci = SataCommandInfo::new((0, 0), (0, 0), 0);
		let bytes: Bytes = handle_removal(
			State(PCFSServerState::new(false, fs, 0)),
			Body(SataRequest::new(mocked_header, mocked_ci, request)),
		)
		.await
		.try_into()
		.expect("Failed to serialize real removal response!");

		assert!(
			!base_dir.exists(),
			"Base directory still exists post 'removal', response:\n\n  {:02X?}\n",
			bytes,
		);
	}

	#[tokio::test]
	pub async fn test_fake_removal() {
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

		let request = SataRemovePacketBody::new(
			base_dir
				.to_str()
				.expect("Test paths must be UTF-8")
				.to_owned(),
		)
		.expect("Failed to create sata remove packet body!");
		let mocked_header = SataPacketHeader::new(0);
		let mocked_ci = SataCommandInfo::new((0, 0), (0, 0), 0);

		let _bytes: Bytes = handle_removal(
			State(PCFSServerState::new(true, fs, 0)),
			Body(SataRequest::new(mocked_header, mocked_ci, request)),
		)
		.await
		.try_into()
		.expect("Failed to serialize real removal response!");
		let renamed_dir = join_many(tempdir.path(), ["a", "b", "c.rm"]);

		assert!(
			!base_dir.exists(),
			"Base directory still exists post 'removal'",
		);
		assert!(renamed_dir.exists(), "Renamed directory doesn't exist?");
	}
}
