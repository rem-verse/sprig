//! Definitions, and handlers for the `Remove` packet type.
//!
//! This does destructive things, and removes files from your filesystem. You
//! can disable the behavior of truly "removing" items from your filesystem,
//! and configure your PCFS client to just move files to `.rm`

use crate::{
	errors::{CatBridgeError, FSError, NetworkParseError},
	fsemul::{
		host_filesystem::ResolvedLocation,
		pcfs::sata_proto::{construct_sata_response, SataPacketHeader},
		HostFilesystem,
	},
};
use bytes::{BufMut, Bytes, BytesMut};
use std::{
	ffi::{CStr, OsStr, OsString},
	path::PathBuf,
};
use tokio::fs::{create_dir_all, read_link, remove_dir_all, remove_file, rename};
use tracing::error;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};
use walkdir::WalkDir;

/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;
/// An error code to send when a path does not exist.
///
/// This is also used in some places that are a bit of a stretch like for
/// network shares on disk space. The path doesn't exist on a disk, so this
/// error code is used, even if it's not quite exact.
const PATH_NOT_EXIST_ERROR: u32 = 0xFFF0_FFE9;

/// A packet to remove a file/directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataRemovePacketBody {
	/// The path to remove, note that this is not the 'resolved' path which is
	/// the path to actual read from.
	///
	/// Interpolation has a few known ways of being replaced:
	///
	/// - `%MLC_EMU_DIR`: `<cafe_sdk>/data/mlc/`
	/// - `%SLC_EMU_DIR`: `<cafe_sdk>/data/slc/`
	/// - `%DISC_EMU_DIR`: `<cafe_sdk>/data/disc/`
	/// - `%SAVE_EMU_DIR`: `<cafe_sdk>/data/save/`
	/// - `%NETWORK`: <mounted network share path>
	path: String,
}

impl SataRemovePacketBody {
	#[must_use]
	pub fn path(&self) -> &str {
		self.path.as_str()
	}

	/// Handle removing a file upon request.
	///
	/// ## Errors
	///
	/// If we cannot construct a sata response packet because our data to send
	/// was somehow too large (this should ideally never happen).
	#[allow(
		// This is far easier to read not collapsed.
		clippy::collapsible_else_if,
	)]
	pub async fn handle(
		&self,
		request_header: &SataPacketHeader,
		actually_do_remove: bool,
		host_filesystem: &HostFilesystem,
	) -> Result<Bytes, CatBridgeError> {
		let Ok(final_location) = host_filesystem.resolve_path(&self.path) else {
			return Self::construct_error(request_header, PATH_NOT_EXIST_ERROR);
		};
		let ResolvedLocation::Filesystem(fs_location) = final_location else {
			todo!("network shares not yet implemented!")
		};

		if fs_location.resolved_path().exists() {
			if actually_do_remove {
				if fs_location.resolved_path().is_file() {
					if let Err(cause) = remove_file(fs_location.resolved_path()).await {
						error!(
						  ?cause,
						  path = %fs_location.resolved_path().display(),
						  "Failed to remove file as requested by PCFS.",
						);
						return Self::construct_error(request_header, FS_ERROR);
					}
				} else if fs_location.resolved_path().is_dir() {
					if let Err(cause) = remove_dir_all(fs_location.resolved_path()).await {
						error!(
						  ?cause,
						  path = %fs_location.resolved_path().display(),
						  "Failed to remove directory as requested by PCFS."
						);
						return Self::construct_error(request_header, FS_ERROR);
					}
				} else {
					return Self::construct_error(request_header, FS_ERROR);
				}
			} else {
				if fs_location.resolved_path().is_file() {
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
						return Self::construct_error(request_header, FS_ERROR);
					}
				} else if fs_location.resolved_path().is_dir() {
					if let Err(cause) = Self::rename_dir(fs_location.resolved_path()).await {
						error!(
						  ?cause,
						  path = %fs_location.resolved_path().display(),
						  "Failed to rename folder (as opposed to remove) as requested by PCFS."
						);
						return Self::construct_error(request_header, FS_ERROR);
					}
				} else {
					return Self::construct_error(request_header, FS_ERROR);
				}
			}
		}

		Ok(construct_sata_response(
			request_header,
			0,
			BytesMut::zeroed(4).freeze(),
		)?)
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
						resolved_path = PathBuf::from(unsafe {
							OsString::from_encoded_bytes_unchecked(new_bytes)
						});
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

	fn construct_error(
		packet_header: &SataPacketHeader,
		error_code: u32,
	) -> Result<Bytes, CatBridgeError> {
		let mut buff = BytesMut::with_capacity(8);
		buff.put_u32(error_code);
		buff.put_u32(0);

		Ok(construct_sata_response(packet_header, 0, buff.freeze())?)
	}
}

impl TryFrom<Bytes> for SataRemovePacketBody {
	type Error = NetworkParseError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x200 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataRemove",
				"Body",
				0x200,
				value.len(),
				value,
			));
		}
		if value.len() > 0x200 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataRemove",
				value.slice(0x200..),
			));
		}

		let path_c_str =
			CStr::from_bytes_until_nul(&value).map_err(NetworkParseError::BadCString)?;

		Ok(Self {
			path: path_c_str.to_str()?.to_owned(),
		})
	}
}

const SATA_REMOVE_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[NamedField::new("path")];

impl Structable for SataRemovePacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataRemovePacketBody",
			Fields::Named(SATA_REMOVE_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataRemovePacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_REMOVE_PACKET_BODY_FIELDS,
			&[Valuable::as_value(&self.path)],
		));
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use crate::fsemul::host_filesystem::test_helpers::{
		create_temporary_host_filesystem, join_many,
	};

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

		let request = SataRemovePacketBody {
			path: base_dir
				.to_str()
				.expect("Test paths must be UTF-8")
				.to_owned(),
		};
		let mocked_header = SataPacketHeader {
			packet_data_len: 0,
			packet_id: 0,
			flags: 0,
			version: 0,
			timestamp_on_host: 0,
			pid_on_host: 0,
		};

		let bytes = request
			.handle(&mocked_header, true, &fs)
			.await
			.expect("Failed to handle removal that was fake!");

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

		let request = SataRemovePacketBody {
			path: base_dir
				.to_str()
				.expect("Test paths must be UTF-8")
				.to_owned(),
		};
		let mocked_header = SataPacketHeader {
			packet_data_len: 0,
			packet_id: 0,
			flags: 0,
			version: 0,
			timestamp_on_host: 0,
			pid_on_host: 0,
		};

		let _ = request
			.handle(&mocked_header, false, &fs)
			.await
			.expect("Failed to handle removal that was fake!");
		let renamed_dir = join_many(tempdir.path(), ["a", "b", "c.rm"]);

		assert!(
			!base_dir.exists(),
			"Base directory still exists post 'removal'",
		);
		assert!(renamed_dir.exists(), "Renamed directory doesn't exist?",);
	}
}
