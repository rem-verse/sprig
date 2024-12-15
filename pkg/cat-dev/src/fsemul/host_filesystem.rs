//! A representation of the filesystem folder we end up serving a cat-dev
//! client.

use crate::{
	errors::{CatBridgeError, FSError},
	fsemul::{bsf::BootSystemFile, dlf::DiskLayoutFile, errors::FSEmulFSError},
	TitleID,
};
use bytes::{Bytes, BytesMut};
use std::path::{Path, PathBuf};
use tokio::fs::{create_dir_all, write as fs_write};
use whoami::username;

/// A wrapper around interacting with the 'host' or PC filesystem for the
/// various times a cat-dev will reach out to the host.
///
/// This is little more than a wrapper around a [`PathBuf`], and targeted
/// methods to make getting files/generating default files/etc. easy. Most of
/// the actual logic for turning a request from `SDIO`, `ATAPI`, etc. all come
/// from those client/server implementations rather than the logic living here.
#[derive(Debug, PartialEq, Eq)]
pub struct HostFilesystem {
	/// The path to the base data directory to serve a filesystem out of.
	cafe_sdk_path: PathBuf,
}

impl HostFilesystem {
	/// Create a filesystem from a root cafe dir.
	///
	/// If no cafe dir is provided, we will attempt to locate the default
	/// installation path for cafe sdk which is:
	///
	/// - `C:\cafe_sdk` on windows.
	/// - `/opt/cafe_sdk` on any unix/bsd like OS.
	///
	/// ## Errors
	///
	/// If the Cafe SDK directory is corrupt, or can't be found. A Cafe SDK
	/// directory is considered corrupt if it is missing core files that we
	/// _need_ to be able to serve a Cafe-OS distribution. These file
	/// requirements may change from version to version of this crate, but should
	/// always be compatible with a clean cafe sdk directory.
	pub fn from_cafe_dir(cafe_dir: Option<PathBuf>) -> Result<Self, FSEmulFSError> {
		let Some(cafe_sdk_path) = cafe_dir.or_else(Self::default_cafe_directory) else {
			return Err(FSEmulFSError::CantFindCafeSdkPath);
		};

		if !Self::join_many(
			&cafe_sdk_path,
			[
				"data", "mlc", "sys", "title", "00050030", "1001000A", "code", "app.xml",
			],
		)
		.exists() || !Self::join_many(
			&cafe_sdk_path,
			[
				"data", "mlc", "sys", "title", "00050030", "1001010A", "code", "app.xml",
			],
		)
		.exists() || !Self::join_many(
			&cafe_sdk_path,
			[
				"data", "mlc", "sys", "title", "00050030", "1001020A", "code", "app.xml",
			],
		)
		.exists()
		{
			return Err(FSEmulFSError::CafeSdkPathCorrupt);
		}

		// Can't generate a `fw.img` file for now :(
		if !Self::join_many(
			&cafe_sdk_path,
			[
				"data", "slc", "sys", "title", "00050010", "1000400A", "code", "fw.img",
			],
		)
		.exists()
		{
			return Err(FSEmulFSError::CafeSdkPathCorrupt);
		}

		Ok(Self { cafe_sdk_path })
	}

	/// The root path to the Cafe SDK.
	///
	/// *note: although we do expose this for logging, and other info... we do
	/// not recommend manually interacting with the SDK path. There are much
	/// better alternatives.*
	#[must_use]
	pub const fn cafe_sdk_path(&self) -> &PathBuf {
		&self.cafe_sdk_path
	}

	/// Get the path to the current boot1 `.bsf` file.
	///
	/// This function will create the boot1 system file, if it does not yet
	/// exist. As a result it may error, if we can't create, and place the
	/// boot system file.
	///
	/// ## Errors
	///
	/// - If the temp directory does not exist, and we can't create it.
	/// - If the boot system file does not exist, and we can't write it to disk.
	pub async fn boot1_sytstem_path(&self) -> Result<PathBuf, FSError> {
		let mut path = self.temp_path().await?;
		path.push("caferun");
		if !path.exists() {
			create_dir_all(&path).await?;
		}
		path.push("ppc.bsf");

		if !path.exists() {
			fs_write(&path, Bytes::from(BootSystemFile::default())).await?;
		}

		Ok(path)
	}

	/// Get the path to the current `diskid.bin`.
	///
	/// If the current Disk ID does not exist, we will write a blank diskid to
	/// this path.
	///
	/// ## Errors
	///
	/// - If the temporary directory does not exist, and we can't create it.
	/// - If the disk ID path does not exist, and we can't write it to disk.
	pub async fn disk_id_path(&self) -> Result<PathBuf, FSError> {
		let mut path = self.temp_path().await?;
		path.push("caferun");
		if !path.exists() {
			create_dir_all(&path).await?;
		}
		path.push("diskid.bin");

		if !path.exists() {
			fs_write(&path, BytesMut::zeroed(32).freeze()).await?;
		}

		Ok(path)
	}

	/// Get the path to the current firmware file to boot on the MION.
	///
	/// This is guaranteed to always exist, as it's part of our check for a
	/// corrupt SDK.
	#[must_use]
	pub fn firmware_file_path(&self) -> PathBuf {
		Self::join_many(
			&self.slc_path_for((0x0005_0010, 0x1000_400A)),
			["code", "fw.img"],
		)
	}

	/// Get the path to the disk layout file for the PPC booting process.
	///
	/// This function will create a disk layout file, as well as a Boot System
	/// File, and a disk id file if they do not yet exist.
	///
	/// ## Errors
	///
	/// - If the temp directory does not exist, and we can't create it.
	/// - If the boot system file does not exist, and we can't write it to disk.
	/// - If the diskid file does not exist, and we can't write it to disk.
	/// - If the firmware image file does not exist.
	/// - If the dlf file does not exist, and we can't create it.
	pub async fn ppc_boot_dlf_path(&self) -> Result<PathBuf, CatBridgeError> {
		let mut path = self.temp_path().await?;
		path.push("caferun");
		if !path.exists() {
			create_dir_all(&path).await.map_err(FSError::from)?;
		}
		path.push("ppc_boot.dlf");

		if !path.exists() {
			// This probably isn't the right set of defaults for everyone, but i'm
			// not yet smart enough to figure all this out.
			let mut root_dlf = DiskLayoutFile::new(0x00B8_8200_u128);
			root_dlf.upsert_addressed_path(0_u128, &self.disk_id_path().await?)?;
			root_dlf.upsert_addressed_path(0x80000_u128, &self.boot1_sytstem_path().await?)?;
			root_dlf.upsert_addressed_path(0x90000_u128, &self.firmware_file_path())?;
			fs_write(&path, Bytes::from(root_dlf))
				.await
				.map_err(FSError::from)?;
		}

		Ok(path)
	}

	/// Get a file from the SLC.
	///
	/// The SLC always serves "sys" files, and are relative to a title id, almost
	/// always a system title id such as (`00050010`).
	///
	/// *note: the file is not guaranteed to exist! It's just a path!*
	#[must_use]
	pub fn slc_path_for(&self, title_id: TitleID) -> PathBuf {
		Self::join_many(
			&self.cafe_sdk_path,
			[
				"data".to_owned(),
				"slc".to_owned(),
				"sys".to_owned(),
				"title".to_owned(),
				format!("{:08X?}", title_id.0),
				format!("{:08X?}", title_id.1),
			],
		)
	}

	/// Get the current path to the temporary directory for this Cafe SDK
	/// install.
	///
	/// ## Errors
	///
	/// - If the temporary path does not exist and could not be created.
	async fn temp_path(&self) -> Result<PathBuf, FSError> {
		let temp_path = Self::join_many(&self.cafe_sdk_path, ["temp".to_owned(), username()]);
		if !temp_path.exists() {
			create_dir_all(&temp_path).await?;
		}
		Ok(temp_path)
	}

	/// A small utility function to join many paths into a single path effeciently.
	#[must_use]
	fn join_many<PathTy, IterTy>(base: &Path, parts: IterTy) -> PathBuf
	where
		PathTy: AsRef<Path>,
		IterTy: IntoIterator<Item = PathTy>,
	{
		let mut as_owned = PathBuf::from(base);
		for part in parts {
			as_owned = as_owned.join(part.as_ref());
		}
		as_owned
	}

	/// Get the current OS's default directory path.
	///
	/// For Windows this is: `C:\cafe_sdk`.
	/// For Unix/BSD likes this is: `/opt/cafe_sdk`
	#[allow(
    // Not actually unreachable unless on unsupported OS.
    unreachable_code,
  )]
	#[must_use]
	pub fn default_cafe_directory() -> Option<PathBuf> {
		#[cfg(target_os = "windows")]
		{
			return Some(PathBuf::from(r"C:\cafe_sdk"));
		}

		#[cfg(any(
			target_os = "linux",
			target_os = "freebsd",
			target_os = "openbsd",
			target_os = "netbsd",
			target_os = "macos"
		))]
		{
			return Some(PathBuf::from("/opt/cafe_sdk"));
		}

		None
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	fn only_accepts_send_sync<T: Send + Sync>(_opt: Option<T>) {}

	#[test]
	pub fn is_send_sync() {
		only_accepts_send_sync::<HostFilesystem>(None);
	}
}
