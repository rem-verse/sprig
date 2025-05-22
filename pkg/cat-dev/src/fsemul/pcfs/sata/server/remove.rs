//! Handle remove packets which can remove files or folders.

use crate::{
	errors::FSError,
	fsemul::{
		host_filesystem::ResolvedLocation,
		pcfs::sata::{
			proto::{SataPacketHeader, SataRemovePacketBody, SataResponse, SataResultCode},
			server::PCFSServerState,
		},
	},
	net::server::requestable::{Body, State},
};
use std::{
	ffi::{OsStr, OsString},
	path::PathBuf,
};
use tokio::fs::{create_dir_all, read_link, remove_dir_all, remove_file, rename};
use tracing::{debug, error};
use walkdir::WalkDir;

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
	request_header: SataPacketHeader,
	State(state): State<PCFSServerState>,
	Body(packet): Body<SataRemovePacketBody>,
) -> SataResponse<SataResultCode> {
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

	if fs_location.resolved_path().exists() {
		if !state.disable_real_removal() {
			if fs_location.resolved_path().is_file() {
				if let Err(cause) = remove_file(fs_location.resolved_path()).await {
					error!(
					  ?cause,
					  path = %fs_location.resolved_path().display(),
					  "Failed to remove file as requested by PCFS.",
					);

					return SataResponse::new(
						state.pid(),
						request_header,
						SataResultCode::error(FS_ERROR),
					);
				}
			} else if fs_location.resolved_path().is_dir() {
				if let Err(cause) = remove_dir_all(fs_location.resolved_path()).await {
					error!(
					  ?cause,
					  path = %fs_location.resolved_path().display(),
					  "Failed to remove directory as requested by PCFS."
					);

					return SataResponse::new(
						state.pid(),
						request_header,
						SataResultCode::error(FS_ERROR),
					);
				}
			} else {
				return SataResponse::new(
					state.pid(),
					request_header,
					SataResultCode::error(FS_ERROR),
				);
			}
		} else if fs_location.resolved_path().is_file() {
			// This should always be fine to do as mount pounts are at most
			// specific to a directory, so moving a file within the same directory
			// doesn't violate the "can't move across mount points" on windows.
			let mut new_filename = fs_location
				.resolved_path()
				.file_name()
				.unwrap_or_default()
				.to_owned();
			new_filename.push(OsStr::new(".rm"));
			let mut new_path = fs_location.resolved_path().clone();
			new_path.pop();
			new_path.push(new_filename);

			if let Err(cause) = rename(fs_location.resolved_path(), new_path).await {
				error!(
				  ?cause,
				  path = %fs_location.resolved_path().display(),
				  "Failed to rename file (as opposed to remove) as requested by PCFS."
				);

				return SataResponse::new(
					state.pid(),
					request_header,
					SataResultCode::error(FS_ERROR),
				);
			}
		} else if fs_location.resolved_path().is_dir() {
			if let Err(cause) = rename_dir(fs_location.resolved_path()).await {
				error!(
				  ?cause,
				  path = %fs_location.resolved_path().display(),
				  "Failed to rename folder (as opposed to remove) as requested by PCFS."
				);

				return SataResponse::new(
					state.pid(),
					request_header,
					SataResultCode::error(FS_ERROR),
				);
			}
		} else {
			return SataResponse::new(state.pid(), request_header, SataResultCode::error(FS_ERROR));
		}
	}

	SataResponse::new(state.pid(), request_header, SataResultCode::success())
}

/// Rename an entire directory.
///
/// We have to implement this ourselves, because [`tokio::fs::rename`], and
/// [`std::fs::rename`] don't support renaming a directory at all on windows,
/// which is one of the critical OS's that we need to support.
///
/// This 'rename' works by actually creating a new directory with the ".rm"
/// added. Then moving all the files over with rename. This is slow, but
/// works.
async fn rename_dir(old_path: &PathBuf) -> Result<(), FSError> {
	let mut new_filename = old_path.file_name().unwrap_or_default().to_owned();
	new_filename.push(OsStr::new(".rm"));
	let mut new_path = old_path.clone();
	new_path.pop();
	new_path.push(new_filename);
	let old_path_bytes = old_path.as_os_str().as_encoded_bytes();
	let new_path_as_str_bytes = new_path.as_os_str().as_encoded_bytes();

	create_dir_all(&new_path).await?;
	for result in WalkDir::new(old_path)
		.follow_links(false)
		.follow_root_links(false)
	{
		let rpb = result?.into_path();
		let os_str_for_entry = rpb.as_os_str().as_encoded_bytes();
		let mut new_bytes = Vec::with_capacity(os_str_for_entry.len() + 3);
		new_bytes.extend_from_slice(new_path_as_str_bytes);
		new_bytes.extend_from_slice(&os_str_for_entry[old_path_bytes.len()..]);
		let as_new_path =
			PathBuf::from(unsafe { OsString::from_encoded_bytes_unchecked(new_bytes) });

		if rpb.is_symlink() {
			let mut resolved_path = read_link(&rpb).await?;
			{
				// Rewrite paths within the directory we're removing.
				let os_str_for_resolved = resolved_path.as_os_str().as_encoded_bytes();
				if os_str_for_resolved.starts_with(old_path_bytes) {
					let mut new_bytes = Vec::with_capacity(os_str_for_resolved.len() + 3);
					new_bytes.extend_from_slice(new_path_as_str_bytes);
					new_bytes.extend_from_slice(&os_str_for_entry[old_path_bytes.len()..]);
					resolved_path =
						PathBuf::from(unsafe { OsString::from_encoded_bytes_unchecked(new_bytes) });
				}
			}

			#[cfg(unix)]
			{
				use std::os::unix::fs::symlink;
				symlink(resolved_path, &as_new_path)?;
			}

			#[cfg(target_os = "windows")]
			{
				use std::os::windows::fs::{symlink_dir, symlink_file};

				if resolved_path.is_dir() {
					symlink_dir(resolved_path, &as_new_path)?;
				} else {
					symlink_file(resolved_path, &as_new_path)?;
				}
			}
		} else if rpb.is_file() {
			rename(&rpb, &as_new_path).await?;
		} else if rpb.is_dir() {
			create_dir_all(as_new_path).await?;
		}
	}

	remove_dir_all(old_path).await?;
	Ok(())
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use crate::fsemul::host_filesystem::test_helpers::{
		create_temporary_host_filesystem, join_many,
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
		let bytes: Bytes = handle_removal(
			mocked_header,
			State(PCFSServerState::new(false, fs, 0)),
			Body(request),
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

		let _bytes: Bytes = handle_removal(
			mocked_header,
			State(PCFSServerState::new(true, fs, 0)),
			Body(request),
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
