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
pub struct FSEmulConfig {
	/// The fully existing configuration as we know it.
	configuration: Ini,
	/// The path we originally loaded ourselves from.
	loaded_from_path: PathBuf,
}

impl FSEmulConfig {
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

	/// Set the current ATAPI Emulation port.
	pub fn set_atapi_emulation_port(&mut self, port: u16) {
		self.configuration
			.set("DEBUG_PORTS", "ATAPI_EMUL", Some(format!("{port}")));
	}

	/// Get the configured debug out port if one has been configured.
	#[must_use]
	pub fn get_debug_out_port(&self) -> Option<u16> {
		self.configuration
			.get("DEBUG_PORTS", "DEBUG_OUT")
			.and_then(|data| match data.parse::<u16>() {
				Ok(value) => Some(value),
				Err(cause) => {
					warn!(
						?cause,
						fsemul.path = %self.loaded_from_path.display(),
						fsemul.section_name = "DEBUG_PORTS",
						fsemul.value_name = "DEBUG_OUT",
						fsemul.value_raw = data,
						"Failed to parse Debug OUT port as number, ignoring!",
					);
					None
				}
			})
	}

	/// Set the current Debug Out port.
	pub fn set_debug_out_port(&mut self, port: u16) {
		self.configuration
			.set("DEBUG_PORTS", "DEBUG_OUT", Some(format!("{port}")));
	}

	/// Get the configured debug control port if one has been configured.
	#[must_use]
	pub fn get_debug_control_port(&self) -> Option<u16> {
		self.configuration
			.get("DEBUG_PORTS", "DEBUG_CONTROL")
			.and_then(|data| match data.parse::<u16>() {
				Ok(value) => Some(value),
				Err(cause) => {
					warn!(
						?cause,
						fsemul.path = %self.loaded_from_path.display(),
						fsemul.section_name = "DEBUG_PORTS",
						fsemul.value_name = "DEBUG_CONTROL",
						fsemul.value_raw = data,
						"Failed to parse Debug CTRL port as number, ignoring!",
					);
					None
				}
			})
	}

	/// Set the current Debug Control port.
	pub fn set_debug_ctrl_port(&mut self, port: u16) {
		self.configuration
			.set("DEBUG_PORTS", "DEBUG_CONTROL", Some(format!("{port}")));
	}

	/// Get the configured HIO out port if one has been configured.
	#[must_use]
	pub fn get_hio_out_port(&self) -> Option<u16> {
		self.configuration
			.get("DEBUG_PORTS", "HIO_OUT")
			.and_then(|data| match data.parse::<u16>() {
				Ok(value) => Some(value),
				Err(cause) => {
					warn!(
						?cause,
						fsemul.path = %self.loaded_from_path.display(),
						fsemul.section_name = "DEBUG_PORTS",
						fsemul.value_name = "HIO_OUT",
						fsemul.value_raw = data,
						"Failed to parse HIO OUT port as number, ignoring!",
					);
					None
				}
			})
	}

	/// Set the current HIO OUT port.
	pub fn set_hio_out_port(&mut self, port: u16) {
		self.configuration
			.set("DEBUG_PORTS", "HIO_OUT", Some(format!("{port}")));
	}

	/// Get the configured PCFS Character port if one has been configured.
	#[must_use]
	pub fn get_pcfs_character_port(&self) -> Option<u16> {
		self.configuration
			.get("DEBUG_PORTS", "CHAR_PCFS")
			.and_then(|data| match data.parse::<u16>() {
				Ok(value) => Some(value),
				Err(cause) => {
					warn!(
						?cause,
						fsemul.path = %self.loaded_from_path.display(),
						fsemul.section_name = "DEBUG_PORTS",
						fsemul.value_name = "CHAR_PCFS",
						fsemul.value_raw = data,
						"Failed to parse PCFS Character port as number, ignoring!",
					);
					None
				}
			})
	}

	/// Set the current PCFS Character port.
	pub fn set_pcfs_character_port(&mut self, port: u16) {
		self.configuration
			.set("DEBUG_PORTS", "CHAR_PCFS", Some(format!("{port}")));
	}

	/// Get the configured PCFS Block port if one has been configured.
	#[must_use]
	pub fn get_pcfs_block_port(&self) -> Option<u16> {
		self.configuration
			.get("DEBUG_PORTS", "PCFS_INOUT")
			.and_then(|data| match data.parse::<u16>() {
				Ok(value) => Some(value),
				Err(cause) => {
					warn!(
						?cause,
						fsemul.path = %self.loaded_from_path.display(),
						fsemul.section_name = "DEBUG_PORTS",
						fsemul.value_name = "PCFS_INOUT",
						fsemul.value_raw = data,
						"Failed to parse PCFS Block port as number, ignoring!",
					);
					None
				}
			})
	}

	/// Set the current PCFS Block port.
	pub fn set_pcfs_block_port(&mut self, port: u16) {
		self.configuration
			.set("DEBUG_PORTS", "PCFS_INOUT", Some(format!("{port}")));
	}

	/// Get the configured Launch Control port if one has been configured.
	#[must_use]
	pub fn get_launch_control_port(&self) -> Option<u16> {
		self.configuration
			.get("DEBUG_PORTS", "LAUNCH_CTRL")
			.and_then(|data| match data.parse::<u16>() {
				Ok(value) => Some(value),
				Err(cause) => {
					warn!(
						?cause,
						fsemul.path = %self.loaded_from_path.display(),
						fsemul.section_name = "DEBUG_PORTS",
						fsemul.value_name = "LAUNCH_CTRL",
						fsemul.value_raw = data,
						"Failed to parse Launch Control port as number, ignoring!",
					);
					None
				}
			})
	}

	/// Set the current Launch Control port.
	pub fn set_launch_control_port(&mut self, port: u16) {
		self.configuration
			.set("DEBUG_PORTS", "LAUNCH_CTRL", Some(format!("{port}")));
	}

	/// Get the configured Net Manage port if one has been configured.
	#[must_use]
	pub fn get_net_manage_port(&self) -> Option<u16> {
		self.configuration
			.get("DEBUG_PORTS", "NET_MANAGE")
			.and_then(|data| match data.parse::<u16>() {
				Ok(value) => Some(value),
				Err(cause) => {
					warn!(
						?cause,
						fsemul.path = %self.loaded_from_path.display(),
						fsemul.section_name = "DEBUG_PORTS",
						fsemul.value_name = "NET_MANAGE",
						fsemul.value_raw = data,
						"Failed to parse Net Manage port as number, ignoring!",
					);
					None
				}
			})
	}

	/// Set the current Net Manage port.
	pub fn set_net_manage_port(&mut self, port: u16) {
		self.configuration
			.set("DEBUG_PORTS", "NET_MANAGE", Some(format!("{port}")));
	}

	/// Get the configured PCFS Sata port if one has been configured.
	#[must_use]
	pub fn get_pcfs_sata_port(&self) -> Option<u16> {
		self.configuration
			.get("DEBUG_PORTS", "PCFS_SATA")
			.and_then(|data| match data.parse::<u16>() {
				Ok(value) => Some(value),
				Err(cause) => {
					warn!(
						?cause,
						fsemul.path = %self.loaded_from_path.display(),
						fsemul.section_name = "DEBUG_PORTS",
						fsemul.value_name = "PCFS_SATA",
						fsemul.value_raw = data,
						"Failed to parse PCFS Sata port as number, ignoring!",
					);
					None
				}
			})
	}

	/// Set the current PCFS Sata port.
	pub fn set_pcfs_sata_port(&mut self, port: u16) {
		self.configuration
			.set("DEBUG_PORTS", "PCFS_SATA", Some(format!("{port}")));
	}

	/// Write the current configuration to disk as a Windows INI file.
	///
	/// We always write the file with carriage returns `\r\n` (windows line
	/// endings), and in UTF-8. So we can always copy-paste the file onto
	/// a windows host and have it be read by the official tools without issue.
	///
	/// ## Errors
	///
	/// If we run into a system error when writing the file to the disk.
	pub async fn write_to_disk(&self) -> Result<(), FSError> {
		let mut serialized_configuration = self.configuration.writes();
		// Multiline is disabled -- so this is safe to check if we have actual carriage returns.
		if !serialized_configuration.contains("\r\n") {
			serialized_configuration = serialized_configuration.replace('\n', "\r\n");
		}

		let parent_dir = {
			let mut path = self.loaded_from_path.clone();
			path.pop();
			path
		};
		tokio::fs::create_dir_all(&parent_dir).await?;

		tokio::fs::write(
			&self.loaded_from_path,
			serialized_configuration.into_bytes(),
		)
		.await?;

		Ok(())
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
			let loaded = FSEmulConfig::load_explicit_path(base_path).await;

			assert!(
				loaded.is_ok(),
				"Failed to load a real original `fsemul.ini`: {:?}",
				loaded,
			);
			let fsemul = loaded.unwrap();

			assert_eq!(fsemul.get_atapi_emulation_port(), None);
			assert_eq!(fsemul.get_debug_out_port(), Some(6001));
			assert_eq!(fsemul.get_debug_control_port(), Some(6002));
			assert_eq!(fsemul.get_hio_out_port(), None);
			assert_eq!(fsemul.get_pcfs_character_port(), None);
			assert_eq!(fsemul.get_pcfs_block_port(), None);
			assert_eq!(fsemul.get_launch_control_port(), None);
			assert_eq!(fsemul.get_net_manage_port(), None);
			assert_eq!(fsemul.get_pcfs_sata_port(), None);
		}
	}

	#[test]
	pub fn can_get_default_path_for_os() {
		assert!(
			FSEmulConfig::get_default_host_path().is_some(),
			"Failed to get default FSEMul.ini path for your os!",
		);
	}

	#[tokio::test]
	pub async fn can_set_and_write_to_file() {
		use tempfile::tempdir;
		use tokio::fs::File;

		let temporary_directory =
			tempdir().expect("Failed to create temporary directory for tests!");
		let mut path = PathBuf::from(temporary_directory.path());
		path.push("fsemul_custom_made.ini");
		{
			File::create(&path)
				.await
				.expect("Failed to create test file to write too!");
		}
		let mut conf = FSEmulConfig::load_explicit_path(path.clone())
			.await
			.expect("Failed to load empty file to write too!");

		conf.set_atapi_emulation_port(8000);
		assert!(conf.write_to_disk().await.is_ok());

		let read_data = String::from_utf8(
			tokio::fs::read(path)
				.await
				.expect("Failed to read written data!"),
		)
		.expect("Written INI file wasn't UTF8?");
		assert_eq!(read_data, "[DEBUG_PORTS]\r\nATAPI_EMUL=8000\r\n");
	}
}
