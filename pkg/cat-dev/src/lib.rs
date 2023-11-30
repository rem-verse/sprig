#![doc = include_str!("../README.md")]
#![allow(
	// I dislike this rule... We import things elsewhere, usually outside of
  // modules themselves.
	clippy::module_name_repetitions,
)]

pub mod errors;
pub mod mion;

use crate::errors::{APIError, FSError};
use configparser::ini::Ini;
use fnv::FnvHashMap;
use std::{
	fmt::{Display, Formatter, Result as FmtResult},
	hash::BuildHasherDefault,
	net::Ipv4Addr,
	path::PathBuf,
};

/// The environment variable name to fetch what version of [`CAFE_HARDWARE`]
/// we're using.
const HARDWARE_ENV_NAME: &str = "CAFE_HARDWARE";
/// The prefix prepended to the bridge name keys in the host env file to
/// prevent collisions.
const BRIDGE_NAME_KEY_PREFIX: &str = "BRIDGE_NAME_";
/// The section name in the ini file we store a list of the host bridges are
/// stored in.
const HOST_BRIDGES_SECTION: &str = "HOST_BRIDGES";
/// The key that contains that stores which bridge is marked as the default.
const DEFAULT_BRIDGE_KEY: &str = "BRIDGE_DEFAULT_NAME";

/// As far as I can derive from the sources available that we can cleanly read
/// (e.g. shell scripts) there are two types of CAT-DEV units. This enum
/// describes those.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum BridgeType {
	/// "MION" is the 'standard' cat-dev unit that most people probably have (and
	/// is in fact the only one the developers have).
	///
	/// MIONs are also refered to as "v3", and "v4". If you have a tan colored
	/// box you almost assuredly have a MION.
	Mion,
	/// A toucan is the other type of cat-dev unit referenced in some of the
	/// shell scripts.
	///
	/// We don't have a ton of documentation on this particular bridge type
	/// mainly because we don't actually own any of these bridge types. They
	/// generally be refered to as "v2" bridges.
	Toucan,
}
impl BridgeType {
	/// Attempt to get the bridge type from the environment.
	///
	/// This will only return `Some()` when the environment variable
	/// `CAFE_HARDWARE` is set to a proper value. If it's not set to a value
	/// then we will always return `None`.
	///
	/// A proper way to always get a bridge type would be doing something similar
	/// to what the scripts do which is fallback to a default:
	///
	/// ```rust
	/// let ty = cat_dev::BridgeType::fetch_bridge_type().unwrap_or_default();
	/// ```
	#[must_use]
	pub fn fetch_bridge_type() -> Option<Self> {
		Self::hardware_type_to_value(std::env::var(HARDWARE_ENV_NAME).as_deref().ok())
	}

	/// Convert a known hardware type to a potential Bridge Type.
	fn hardware_type_to_value(hardware_type: Option<&str>) -> Option<Self> {
		match hardware_type {
			Some("ev") => Some(Self::Toucan),
			Some("ev_x4") => Some(Self::Mion),
			Some(val) => {
				if val.chars().skip(6).collect::<String>() == *"mp" {
					Some(Self::Mion)
				} else if let Some(num) = val
					.chars()
					.nth(6)
					.and_then(|character| char::to_digit(character, 10))
				{
					if num <= 2 {
						Some(Self::Toucan)
					} else {
						Some(Self::Mion)
					}
				} else {
					None
				}
			}
			_ => None,
		}
	}
}
impl Default for BridgeType {
	/// Shell scripts default to using MION if all things are the same.
	///
	/// In general you probably want to use [`BridgeType::fetch_bridge_type`],
	/// and should only use this default when using something like
	/// `unwrap_or_default` in case the bridge type isn't in the environment.
	fn default() -> Self {
		BridgeType::Mion
	}
}
impl Display for BridgeType {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		match *self {
			Self::Mion => write!(fmt, "Mion"),
			Self::Toucan => write!(fmt, "Toucan"),
		}
	}
}

/// The state of bridges that are actively known on this host.
///
/// This is effectively just a list of bridges along with one of those being
/// set as the "default" bridge. A lot of this configuration is stored within
/// a system level directory, see [`BridgeHostState::get_default_host_path`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeHostState {
	/// The fully existing configuration as we know it.
	configuration: Ini,
	/// The path we originally loaded ourselves from.
	loaded_from_path: PathBuf,
}
impl BridgeHostState {
	/// Attempt to load the bridge host state from the filesystem.
	///
	/// This is commonly referred to as `bridge_env.ini`, stored normally in
	/// Windows under the `%APPDATA%\bridge_env.ini` file. This is where tools
	/// like `getbridge`/`hostdisplayversion`/`setbridge` store which bridges
	/// you actually use, and which one you've set as the default bridge.
	///
	/// ## Errors
	///
	/// - If we cannot get the default host path for your OS.
	/// - Any error case from [`BridgeHostState::load_explicit_path`].
	pub async fn load() -> Result<Self, FSError> {
		let default_host_path =
			Self::get_default_host_path().ok_or(FSError::CantFindHostEnvPath)?;
		Self::load_explicit_path(default_host_path).await
	}

	/// Attempt to load the bridge host state file from the filesystem.
	///
	/// This is commonly referred to as `bridge_env.ini`, and is a small
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

	/// Grab the currently configured default host bridge.
	///
	/// This returns an option (representing if any default has been configured),
	/// and then will return an option for the ip address as the name could
	/// potentially point to a name that doesn't have an ip address set, or one
	/// that doesn't have a valid IPv4 address (as bridges are required to be
	/// IPv4).
	#[must_use]
	pub fn get_default_bridge(&self) -> Option<(String, Option<Ipv4Addr>)> {
		if let Some(host_key) = self
			.configuration
			.get(HOST_BRIDGES_SECTION, DEFAULT_BRIDGE_KEY)
		{
			let host_name = host_key
				.as_str()
				.trim_start_matches(BRIDGE_NAME_KEY_PREFIX)
				.to_owned();

			Some((
				host_name,
				self.configuration
					.get(HOST_BRIDGES_SECTION, &host_key)
					.and_then(|value| value.parse::<Ipv4Addr>().ok()),
			))
		} else {
			None
		}
	}

	/// Get an actively configured bridge.
	///
	/// Returns `(BridgeIP, IsDefault)`, if a bridge is actively configured.
	#[must_use]
	pub fn get_bridge(&self, bridge_name: &str) -> Option<(Option<Ipv4Addr>, bool)> {
		let default_key = self
			.configuration
			.get(HOST_BRIDGES_SECTION, DEFAULT_BRIDGE_KEY);
		let key = format!("{BRIDGE_NAME_KEY_PREFIX}{bridge_name}");
		let is_default = default_key.as_deref() == Some(key.as_str());

		self.configuration
			.get(HOST_BRIDGES_SECTION, &key)
			.map(|value| (value.parse::<Ipv4Addr>().ok(), is_default))
	}

	/// List all the bridges that are actively configured.
	///
	/// Returns a map of `<BridgeName, (BridgeIP, IsDefault)>`. The Bridge IP
	/// will be an empty option if we could not parse the value as an IPv4
	/// Address (i.e. the value is invalid), or the key did not have a value.
	#[must_use]
	pub fn list_bridges(&self) -> FnvHashMap<String, (Option<Ipv4Addr>, bool)> {
		let ini_data = self.configuration.get_map_ref();
		let Some(host_bridge_section) = ini_data.get(HOST_BRIDGES_SECTION) else {
			return FnvHashMap::with_capacity_and_hasher(0, BuildHasherDefault::default());
		};

		let default_key = if let Some(Some(value)) = host_bridge_section.get(DEFAULT_BRIDGE_KEY) {
			Some(value)
		} else {
			None
		};

		let mut bridges = FnvHashMap::default();
		for (key, value) in host_bridge_section {
			if key.as_str() == DEFAULT_BRIDGE_KEY
				|| !key.as_str().starts_with(BRIDGE_NAME_KEY_PREFIX)
			{
				continue;
			}

			let is_default = Some(key) == default_key;
			bridges.insert(
				key.trim_start_matches(BRIDGE_NAME_KEY_PREFIX).to_owned(),
				(
					value.as_ref().and_then(|val| val.parse::<Ipv4Addr>().ok()),
					is_default,
				),
			);
		}
		bridges
	}

	/// Insert a new bridge, or update it's value.
	///
	/// *note: this will be visible in memory immediately, but in order to
	/// persist it, or have it seen in another process you need to call
	/// [`BridgeHostState::write_to_disk`].*
	///
	/// ## Errors
	///
	/// If the bridge name is not ascii.
	/// If the bridge name is empty.
	/// If the bridge name is too long.
	pub fn upsert_bridge(
		&mut self,
		bridge_name: &str,
		bridge_ip: Ipv4Addr,
	) -> Result<(), APIError> {
		if !bridge_name.is_ascii() {
			return Err(APIError::DeviceNameMustBeAscii);
		}
		if bridge_name.is_empty() {
			return Err(APIError::DeviceNameCannotBeEmpty);
		}
		if bridge_name.len() > 255 {
			return Err(APIError::DeviceNameTooLong(bridge_name.len()));
		}

		self.configuration.set(
			HOST_BRIDGES_SECTION,
			&format!("{BRIDGE_NAME_KEY_PREFIX}{bridge_name}"),
			Some(format!("{bridge_ip}")),
		);
		Ok(())
	}

	/// Remove a bridge from the configuration file.
	///
	/// *note: this will be visible in memory immediately, but in order to
	/// persist it, or have it seen in another process you need to call
	/// [`BridgeHostState::write_to_disk`].*
	pub fn remove_bridge(&mut self, bridge_name: &str) {
		self.configuration.remove_key(
			HOST_BRIDGES_SECTION,
			&format!("{BRIDGE_NAME_KEY_PREFIX}{bridge_name}"),
		);
	}

	/// Remove the default bridge key from the configuration file.
	///
	/// *note: this will be visible in memory immediately, but in order to
	/// persist it, or have it seen in another process you need to call
	/// [`BridgeHostState::write_to_disk`].*
	pub fn remove_default_bridge(&mut self) {
		self.configuration
			.remove_key(HOST_BRIDGES_SECTION, DEFAULT_BRIDGE_KEY);
	}

	/// Set the default bridge for your host.
	///
	/// ## Errors
	///
	/// If you try setting the default bridge to a bridge that does not exist.
	/// If your device name is not ascii.
	/// If your device name is empty.
	/// If your device name is too long.
	pub fn set_default_bridge(&mut self, bridge_name: &str) -> Result<(), APIError> {
		if !bridge_name.is_ascii() {
			return Err(APIError::DeviceNameMustBeAscii);
		}
		if bridge_name.is_empty() {
			return Err(APIError::DeviceNameCannotBeEmpty);
		}
		if bridge_name.len() > 255 {
			return Err(APIError::DeviceNameTooLong(bridge_name.len()));
		}

		let bridge_key = format!("{BRIDGE_NAME_KEY_PREFIX}{bridge_name}");
		if self
			.configuration
			.get(HOST_BRIDGES_SECTION, &bridge_key)
			.is_none()
		{
			return Err(APIError::DefaultDeviceMustExist);
		}

		self.configuration
			.set(HOST_BRIDGES_SECTION, DEFAULT_BRIDGE_KEY, Some(bridge_key));

		Ok(())
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

	/// Get the path the Bridge Host State file was being loaded from.
	#[must_use]
	pub fn get_path(&self) -> &PathBuf {
		&self.loaded_from_path
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
			use std::env::var as env_var;
			if let Ok(appdata_dir) = env_var("APPDATA") {
				let mut path = PathBuf::from(appdata_dir);
				path.push("bridge_env.ini");
				return Some(path);
			}

			return None;
		}

		#[cfg(target_os = "macos")]
		{
			use std::env::var as env_var;
			if let Ok(home_dir) = env_var("HOME") {
				let mut path = PathBuf::from(home_dir);
				path.push("Library");
				path.push("Application Support");
				path.push("bridge_env.ini");
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
				path.push("bridge_env.ini");
				return Some(path);
			} else if let Ok(home_dir) = env_var("HOME") {
				let mut path = PathBuf::from(home_dir);
				path.push(".config");
				path.push("bridge_env.ini");
				return Some(path);
			}

			return None;
		}

		None
	}
}
impl Default for BridgeHostState {
	fn default() -> Self {
		Self {
			configuration: Ini::new(),
			loaded_from_path: Self::get_default_host_path()
				.unwrap_or(PathBuf::from("bridge_env.ini")),
		}
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[test]
	pub fn bridge_type_parsing() {
		// No values get mapped to none.
		assert_eq!(
			BridgeType::hardware_type_to_value(None),
			None,
			"Empty hardware type did not map to a null bridge type?"
		);

		// Static Toucan & MION values.
		assert_eq!(
			BridgeType::hardware_type_to_value(Some("ev")),
			Some(BridgeType::Toucan),
			"Hardware type `ev` was not a `Toucan` bridge!"
		);
		assert_eq!(
			BridgeType::hardware_type_to_value(Some("ev_x4")),
			Some(BridgeType::Mion),
			"Hardware type `ev_x4` was not a `Mion` bridge!",
		);

		// Parsed Toucan & MION Values.
		assert_eq!(
			BridgeType::hardware_type_to_value(Some("catdevmp")),
			Some(BridgeType::Mion),
			"Hardware type `catdevmp` was not a `Mion` bridge!"
		);
		assert_eq!(
			BridgeType::hardware_type_to_value(Some("catdev200")),
			Some(BridgeType::Toucan),
			"Hardware type `catdev200` was not a `Toucan` bridge!"
		);

		// Check that we don't just do an endswith mp:
		assert_eq!(
			BridgeType::hardware_type_to_value(Some("catdevdevmp")),
			None,
			"Invalid hardware type did not get mapped to an empty bridge!",
		);
	}

	#[test]
	pub fn bridge_type_default_is_mion() {
		assert_eq!(
			BridgeType::default(),
			BridgeType::Mion,
			"Default bridge type was not mion!"
		);
	}

	#[test]
	pub fn can_find_host_env() {
		assert!(
			BridgeHostState::get_default_host_path().is_some(),
			"Failed to find the host state path for your particular OS, please file an issue!",
		);
	}

	#[tokio::test]
	pub async fn can_load_ini_files() {
		// First test loading the actual real configuration file completely not
		// touched by our tools, and just rsync'd into the source tree from the
		// real machine.
		let mut test_data_dir = PathBuf::from(
			std::env::var("CARGO_MANIFEST_DIR")
				.expect("Failed to read `CARGO_MANIFEST_DIR` to locate test files!"),
		);
		test_data_dir.push("src");
		test_data_dir.push("test-data");

		{
			let mut real_config_path = test_data_dir.clone();
			real_config_path.push("real-bridge-env.ini");
			let real_config = BridgeHostState::load_explicit_path(real_config_path)
				.await
				.expect("Failed to load a real `bridge_env.ini`!");

			let all_bridges = real_config.list_bridges();
			assert_eq!(
				all_bridges.len(),
				1,
				"Didn't find the single bridge that should've been in our real life `bridge_env.ini`!",
			);
			assert_eq!(
				all_bridges.get("00-25-5C-BA-5A-00").cloned(),
				Some((Some(Ipv4Addr::new(192, 168, 7, 40)), true)),
				"Failed to find a default bridge returned through listed bridges.",
			);
			assert_eq!(
				real_config.get_default_bridge(),
				Some((
					"00-25-5C-BA-5A-00".to_owned(),
					Some(Ipv4Addr::new(192, 168, 7, 40)),
				)),
				"Failed to get default bridge"
			);
			assert_eq!(
				real_config.get_bridge("00-25-5C-BA-5A-00"),
				Some((Some(Ipv4Addr::new(192, 168, 7, 40)), true))
			);
		}

		{
			let mut real_config_path = test_data_dir.clone();
			real_config_path.push("fake-valid-bridge-env.ini");
			let real_config = BridgeHostState::load_explicit_path(real_config_path)
				.await
				.expect("Failed to load a real `bridge_env.ini`!");

			assert_eq!(
				real_config.get_default_bridge(),
				Some((
					"00-25-5C-BA-5A-00".to_owned(),
					Some(Ipv4Addr::new(192, 168, 7, 40))
				)),
			);
			let all_bridges = real_config.list_bridges();
			assert_eq!(
				all_bridges.len(),
				3,
				"Didn't find the three bridge that should've been in our fake but valid `bridge_env.ini`!",
			);
			assert_eq!(
				all_bridges.get("00-25-5C-BA-5A-00").cloned(),
				Some((Some(Ipv4Addr::new(192, 168, 7, 40)), true)),
				"Failed to find a default bridge returned through listed bridges in fake but valid bridge env.",
			);
			assert_eq!(
				all_bridges.get("00-25-5C-BA-5A-01").cloned(),
				Some((Some(Ipv4Addr::new(192, 168, 7, 41)), false)),
				"Failed to find a non-default bridge returned through listed bridges in fake but valid bridge env.",
			);
			assert_eq!(
				all_bridges.get("00-25-5C-BA-5A-02").cloned(),
				Some((None, false)),
				"Failed to find a non-default bridge returned through listed bridges in fake but valid bridge env.",
			);
		}

		{
			let mut real_config_path = test_data_dir.clone();
			real_config_path.push("default-but-no-value.ini");
			let real_config = BridgeHostState::load_explicit_path(real_config_path)
				.await
				.expect("Failed to load a real `bridge_env.ini`!");

			assert_eq!(
				real_config.get_default_bridge(),
				Some(("00-25-5C-BA-5A-01".to_owned(), None)),
			);
			let all_bridges = real_config.list_bridges();
			assert_eq!(
				all_bridges.get("00-25-5C-BA-5A-00").cloned(),
				Some((Some(Ipv4Addr::new(192, 168, 7, 40)), false)),
				"Failed to find a default bridge returned through listed bridges in bridge env with default but no value.",
			);
		}

		{
			let mut real_config_path = test_data_dir.clone();
			real_config_path.push("default-but-invalid-value.ini");
			let real_config = BridgeHostState::load_explicit_path(real_config_path)
				.await
				.expect("Failed to load a real `bridge_env.ini`!");

			assert_eq!(
				real_config.get_default_bridge(),
				Some(("00-25-5C-BA-5A-00".to_owned(), None)),
			);
			let all_bridges = real_config.list_bridges();
			assert_eq!(
				all_bridges.get("00-25-5C-BA-5A-00").cloned(),
				Some((None, true)),
			);
		}

		{
			let mut real_config_path = test_data_dir.clone();
			real_config_path.push("invalid-ini-file.ini");

			assert!(matches!(
				BridgeHostState::load_explicit_path(real_config_path).await,
				Err(FSError::InvalidDataNeedsToBeINI(_)),
			));
		}
	}

	#[tokio::test]
	pub async fn can_set_and_write_to_file() {
		use std::fs::File;
		use tempfile::tempdir;

		let temporary_directory =
			tempdir().expect("Failed to create temporary directory for tests!");
		let mut path = PathBuf::from(temporary_directory.path());
		path.push("bridge_env_custom_made.ini");
		{
			File::create(&path).expect("Failed to create test file to write too!");
		}
		let mut host_env = BridgeHostState::load_explicit_path(path.clone())
			.await
			.expect("Failed to load empty file to write too!");

		assert_eq!(
			host_env.set_default_bridge("00-25-5C-BA-5A-00"),
			Err(APIError::DefaultDeviceMustExist),
		);
		assert_eq!(
			host_env.upsert_bridge("", Ipv4Addr::new(192, 168, 1, 1)),
			Err(APIError::DeviceNameCannotBeEmpty),
		);
		assert_eq!(
			host_env.upsert_bridge("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", Ipv4Addr::new(192, 168, 1, 1)),
			Err(APIError::DeviceNameTooLong(256)),
		);
		assert_eq!(
			host_env.upsert_bridge("𒀀", Ipv4Addr::new(192, 168, 1, 1)),
			Err(APIError::DeviceNameMustBeAscii),
		);

		assert!(host_env
			.upsert_bridge(" with spaces ", Ipv4Addr::new(192, 168, 1, 1))
			.is_ok());
		assert!(host_env.set_default_bridge(" with spaces ").is_ok());
		assert!(host_env
			.upsert_bridge("00-25-5C-BA-5A-00", Ipv4Addr::new(192, 168, 1, 2))
			.is_ok());
		assert!(host_env
			.upsert_bridge(" with spaces ", Ipv4Addr::new(192, 168, 1, 3))
			.is_ok());
		assert!(host_env.set_default_bridge("00-25-5C-BA-5A-00").is_ok());
		assert!(host_env.write_to_disk().await.is_ok());

		let read_data = String::from_utf8(
			tokio::fs::read(path)
				.await
				.expect("Failed to read written data!"),
		)
		.expect("Written INI file wasn't UTF8?");
		// Ordering isn't guaranteed has to be one of these!
		let choices = [
			"[HOST_BRIDGES]\r\nBRIDGE_NAME_00-25-5C-BA-5A-00=192.168.1.2\r\nBRIDGE_DEFAULT_NAME=BRIDGE_NAME_00-25-5C-BA-5A-00\r\nBRIDGE_NAME_ with spaces =192.168.1.3\r\n".to_owned(),
			"[HOST_BRIDGES]\r\nBRIDGE_NAME_00-25-5C-BA-5A-00=192.168.1.2\r\nBRIDGE_NAME_ with spaces =192.168.1.3\r\nBRIDGE_DEFAULT_NAME=BRIDGE_NAME_00-25-5C-BA-5A-00\r\n".to_owned(),
			"[HOST_BRIDGES]\r\nBRIDGE_NAME_ with spaces =192.168.1.3\r\nBRIDGE_NAME_00-25-5C-BA-5A-00=192.168.1.2\r\nBRIDGE_DEFAULT_NAME=BRIDGE_NAME_00-25-5C-BA-5A-00\r\n".to_owned(),
			"[HOST_BRIDGES]\r\nBRIDGE_NAME_ with spaces =192.168.1.3\r\nBRIDGE_DEFAULT_NAME=BRIDGE_NAME_00-25-5C-BA-5A-00\r\nBRIDGE_NAME_00-25-5C-BA-5A-00=192.168.1.2\r\n".to_owned(),
			"[HOST_BRIDGES]\r\nBRIDGE_DEFAULT_NAME=BRIDGE_NAME_00-25-5C-BA-5A-00\r\nBRIDGE_NAME_ with spaces =192.168.1.3\r\nBRIDGE_NAME_00-25-5C-BA-5A-00=192.168.1.2\r\n".to_owned(),
			"[HOST_BRIDGES]\r\nBRIDGE_DEFAULT_NAME=BRIDGE_NAME_00-25-5C-BA-5A-00\r\nBRIDGE_NAME_00-25-5C-BA-5A-00=192.168.1.2\r\nBRIDGE_NAME_ with spaces =192.168.1.3\r\n".to_owned(),
		];

		if !choices.contains(&read_data) {
			panic!("Unexpected host bridges ini file:\n{read_data}");
		}
	}
}
