//! Code related to filesystem emulation.
//!
//! It should be noted there are two common terms when talking about file
//! system emulation.
//!
//! - `FSEmul` is the core process that talks with the MION, and talks with
//!   the MION to implement effectively all of the actual protocols.
//! - `PCFS`/`PCFSServer` are tools built _on-top_ of `FSEmul`, and contain
//!   their own protocols to implement a filesystem on your PC.
//!
//! This is meant to be an all encompassing set of utilities related to
//! file-system emulation, and as a result covers _both_ `FSEmul` & `PCFS`.

pub mod atapi;
pub mod bsf;
pub mod dlf;
pub mod errors;
mod host_filesystem;
pub mod pcfs;
pub mod sdio;

use crate::{errors::FSError, fsemul::errors::FSEmulFSError};
use configparser::ini::Ini;
use std::path::PathBuf;
use tracing::warn;

pub use host_filesystem::HostFilesystem;

/// Active configuration for file-system emulation.
///
/// This is generally soter along with the host-bridge configuration in a
/// simple ini file without much configuration. All official nintendo tools
/// will read from this file as opposed to querying the actual device itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsEmulConfig {
	/// The fully existing configuration as we know it.
	configuration: Ini,
	/// The path we originally loaded ourselves from.
	loaded_from_path: PathBuf,
}

impl FsEmulConfig {
	/// Attempt to load the fsemul configuration from the filesystem.
	///
	/// This is commonly referred to as `fsemul.ini`, stored normally in
	/// Windows under the `C:\Program Files\Nintendo\HostBridge\fsemul.ini`
	/// file. This is where tools like `fsemul` store which ports are being used,
	/// and where things like Session Manager ports happen.
	///
	/// ## Errors
	///
	/// - If we cannot get the default host path for your OS.
	/// - Any error case from [`FSEmulConfig::load_explicit_path`].
	pub async fn load() -> Result<Self, FSError> {
		let default_host_path = Self::get_default_host_path().ok_or(FSEmulFSError::CantFindPath)?;
		Self::load_explicit_path(default_host_path).await
	}

	/// Attempt to load the fsemul configuration file from the filesystem.
	///
	/// This is commonly referred to as `fsemul.ini`, and is a small
	/// Windows (so UTF8) ini file, separated by newlines being `\r\n`.
	///
	/// ## Errors
	///
	/// - If we cannot read from the file on the file system.
	/// - If we cannot parse the data in the file as UTF8.
	/// - If we cannot parse the data as an INI file.
	pub async fn load_explicit_path(path: PathBuf) -> Result<Self, FSError> {
		if path.exists() {
			let as_bytes = tokio::fs::read(&path).await?;
			let as_string = String::from_utf8(as_bytes)?;

			let mut ini_contents = Ini::new_cs();
			ini_contents
				.read(as_string)
				.map_err(|ini_error| FSError::InvalidDataNeedsToBeINI(format!("{ini_error:?}")))?;

			Ok(Self {
				configuration: ini_contents,
				loaded_from_path: path,
			})
		} else {
			Ok(Self {
				configuration: Ini::new_cs(),
				loaded_from_path: path,
			})
		}
	}

	/// Get the configured ATAPI Emulation port if one has been configured.
	#[must_use]
	pub fn get_atapi_emulation_port(&self) -> Option<u16> {
		self.configuration
			.get("DEBUG_PORTS", "ATAPI_EMUL")
			.and_then(|data| match data.parse::<u16>() {
				Ok(value) => Some(value),
				Err(cause) => {
					warn!(
						?cause,
						fsemul.path = %self.loaded_from_path.display(),
						fsemul.section_name = "DEBUG_PORTS",
						fsemul.value_name = "ATAPI_EMUL",
						fsemul.value_raw = data,
						"Failed to parse ATAPI Emulation port as number, ignoring!",
					);
					None
				}
			})
	}

	/// Get the default path that the bridge host state is supposed to be stored
	/// in.
	///
	/// NOTE: this directory is not necissarily guaranteed to exist.
	///
	/// Returns none when we can't find an appropriate path to store bridge host
	/// state in.
	#[allow(
		// We explicitly use cfg blocks to block all escape.
		//
		// However, if you're on a non explicitly mentioned OS, we still want the
		// fallback.
		unreachable_code,
	)]
	#[must_use]
	pub fn get_default_host_path() -> Option<PathBuf> {
		#[cfg(target_os = "windows")]
		{
			return Some(PathBuf::from(
				r"C:\Program Files\Nintendo\HostBridge\fsemul.ini",
			));
		}

		#[cfg(target_os = "macos")]
		{
			use std::env::var as env_var;
			if let Ok(home_dir) = env_var("HOME") {
				let mut path = PathBuf::from(home_dir);
				path.push("Library");
				path.push("Application Support");
				path.push("Nintendo");
				path.push("HostBridge");
				path.push("fsemul.ini");
				return Some(path);
			}

			return None;
		}

		#[cfg(any(
			target_os = "linux",
			target_os = "freebsd",
			target_os = "openbsd",
			target_os = "netbsd"
		))]
		{
			use std::env::var as env_var;
			if let Ok(xdg_config_dir) = env_var("XDG_CONFIG_HOME") {
				let mut path = PathBuf::from(xdg_config_dir);
				path.push("Nintendo");
				path.push("HostBridge");
				path.push("fsemul.ini");
				return Some(path);
			} else if let Ok(home_dir) = env_var("HOME") {
				let mut path = PathBuf::from(home_dir);
				path.push(".config");
				path.push("Nintendo");
				path.push("HostBridge");
				path.push("fsemul.ini");
				return Some(path);
			}

			return None;
		}

		None
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[tokio::test]
	pub async fn can_load_ini_files() {
		let mut test_data_dir = PathBuf::from(
			std::env::var("CARGO_MANIFEST_DIR")
				.expect("Failed to read `CARGO_MANIFEST_DIR` to locate test files!"),
		);
		test_data_dir.push("src");
		test_data_dir.push("fsemul");
		test_data_dir.push("test-data");

		// Real actual fsemul configuration file I had.
		{
			let mut base_path = test_data_dir.clone();
			base_path.push("orig-fsemul.ini");
			let loaded = FsEmulConfig::load_explicit_path(base_path).await;

			assert!(
				loaded.is_ok(),
				"Failed to load a real original `fsemul.ini`: {:?}",
				loaded,
			);
		}
	}
}
/*
Known unhandled config items:

```ini
[DEBUG_PORTS]
DEBUG_OUT
DEBUG_CONTROL
HIO_OUT
CHAR_PCFS
PCFS_INOUT
LAUNCH_CTRL
NET_MANAGE
PCFS_SATA
```
*/
