//! Common wrapper around the bridge configuration aka the [`FSEmulConfig`].
//!
//! Some commands may need to access the configuration file for bridges, this
//! could be for doing things like looking up the default bridge, this could be
//! for setting the default bridge, and things like that. We ideally would only
//! ever open the file once, and read it once. In order to achieve that goal we
//! have these series of functions which wrap around a static safely.

use crate::{
	exit_codes::{
		ARGV_CAFE_ROOT_LOAD_FAILURE, ARGV_FSEMUL_LOAD_FAILURE, ARGV_NO_CAFE_ROOT,
		ARGV_NO_FSEMUL_PATH, HOST_FILESYSTEM_INIT_FAILURE,
	},
	knobs::{
		cli::FSEmulConfigurationFlags,
		env::{CAFE_ROOT, FSMEUL_CONFIG_PATH as FSEMUL_CONFIG_PATH_ENV_ARG},
	},
};
use cat_dev::fsemul::{FSEmulConfig, HostFilesystem};
use std::{
	path::PathBuf,
	sync::{
		OnceLock,
		atomic::{AtomicBool, Ordering},
	},
};
use tokio::sync::{RwLock, RwLockMappedWriteGuard, RwLockReadGuard, RwLockWriteGuard};
use tracing::{error, field::valuable, info};

static FSEMUL_CONFIG: RwLock<Option<FSEmulConfig>> = RwLock::const_new(None);
static HOST_FILE_SYSTEM: OnceLock<HostFilesystem> = OnceLock::new();

static FSEMUL_CONFIG_PATH: RwLock<Option<PathBuf>> = RwLock::const_new(None);
static CAFE_DATA_PATH: RwLock<Option<PathBuf>> = RwLock::const_new(None);
static FORCE_UNIQUE_FDS: AtomicBool = AtomicBool::new(false);

/// Initialize all of the stuff necessary for fetching from the bridge
/// configuration file.
///
/// This just sets everything up, it doesn't actually open the bridge
/// configuration file, until someone actually requests it for the first time.
pub async fn initialize_fsemul_config(fsemul_config_flags: &FSEmulConfigurationFlags) {
	let fsemul_default_path = FSEmulConfig::get_default_host_path();
	if fsemul_default_path.is_none() {
		info!(
			id = "bridgectl::argv::no_default_fsemul_config_path",
			"looks like your OS doesn't have a default fsemul config path, please file an issue to support your OS better",
		);
	}
	let host_default_path = HostFilesystem::default_cafe_folder();
	if host_default_path.is_none() {
		info!(
			id = "bridgectl::argv::no_default_host_filesystem_path",
			"looks like your OS doesn't have a default host filesystem path, please file an issue to support your OS better",
		);
	}
	if fsemul_config_flags.force_unique_fds() {
		FORCE_UNIQUE_FDS.store(true, Ordering::SeqCst);
	}

	if let Some(cli_arg) = fsemul_config_flags.fsemul_config_path() {
		let mut locked_env_path = FSEMUL_CONFIG_PATH.write().await;
		_ = locked_env_path.insert(cli_arg.clone());
	} else if let Some(env_arg) = FSEMUL_CONFIG_PATH_ENV_ARG.as_ref() {
		let mut locked_env_path = FSEMUL_CONFIG_PATH.write().await;
		_ = locked_env_path.insert(env_arg.clone());
	} else if let Some(default_path) = fsemul_default_path {
		let mut locked_env_path = FSEMUL_CONFIG_PATH.write().await;
		_ = locked_env_path.insert(default_path);
	}

	if let Some(cli_arg) = fsemul_config_flags.cafe_dir() {
		let mut locked_env_path = CAFE_DATA_PATH.write().await;
		_ = locked_env_path.insert(cli_arg.clone());
	} else if let Some(env_arg) = CAFE_ROOT.as_ref() {
		let mut locked_env_path = CAFE_DATA_PATH.write().await;
		_ = locked_env_path.insert(env_arg.clone());
	} else if let Some(default_path) = host_default_path {
		let mut locked_env_path = CAFE_DATA_PATH.write().await;
		_ = locked_env_path.insert(default_path);
	}
}

/// Optionally lease the file system emulation configuration if it was able
/// to be parsed, but you don't want to exit if it's not present.
pub async fn lease_fsemul_config_optionally<'lf>() -> Option<RwLockReadGuard<'lf, FSEmulConfig>> {
	try_to_load_fsemul_config().await;

	let read_lock = FSEMUL_CONFIG.read().await;
	if read_lock.is_some() {
		Some(RwLockReadGuard::map(read_lock, |inner_guard| {
			inner_guard.as_ref().expect("impossible")
		}))
	} else {
		None
	}
}

/// Optionally lease the cafe root configuration if it was able
/// to be parsed, but you don't want to exit if it's not present.
#[allow(unused)]
pub async fn lease_host_file_system_optionally() -> Option<&'static HostFilesystem> {
	try_to_load_host_file_system().await;
	HOST_FILE_SYSTEM.get()
}

/// Get a non-mutable reference to the current filesystem configuration.
///
/// This will exit the program if for some reason we can't load the bridge
/// configuration file from the disk.
#[allow(unused)]
pub async fn lease_fsemul_config<'lf>() -> RwLockReadGuard<'lf, FSEmulConfig> {
	validate_fsemul_config_is_populated().await;

	RwLockReadGuard::map(FSEMUL_CONFIG.read().await, |inner_guard| {
		inner_guard.as_ref().expect("impossible")
	})
}

/// Get a non-mutable reference to the current host filesystem.
///
/// This will exit the program if for some reason we can't load the host
/// filesystem from the disk.
pub async fn lease_host_file_system() -> &'static HostFilesystem {
	validate_host_file_system_is_populated().await;
	HOST_FILE_SYSTEM.get().expect("impossible")
}

/// Get a mutable reference to the current filesystem configuration.
///
/// This will exit the program if for some reason we can't load the filesystem
/// configuration file from the disk.
#[allow(unused)]
pub async fn lease_fsemul_config_mut<'lf>() -> RwLockMappedWriteGuard<'lf, FSEmulConfig> {
	validate_fsemul_config_is_populated().await;

	RwLockWriteGuard::map(FSEMUL_CONFIG.write().await, |inner_guard| {
		inner_guard.as_mut().expect("impossible")
	})
}

async fn try_to_load_fsemul_config() {
	let read_lock = FSEMUL_CONFIG.read().await;
	let exists = read_lock.is_some();
	std::mem::drop(read_lock);

	if !exists {
		let mut write_lock = FSEMUL_CONFIG.write().await;
		// It's possible while we were waiting to acquire the exclusive write lock
		// that someone else populated the value. If so, let's just return.
		if write_lock.is_some() {
			return;
		}

		let read_env_path = FSEMUL_CONFIG_PATH.read().await;
		if !read_env_path.is_some() {
			error!(
				id = "bridgectl::argv::bridge_state_path_required",
				cause = "Could not find the bridge state path aka `bridge_env.ini`",
				suggestions = valuable(&[
					"You can specify the path manually with an environment variable: [`BRIDGECTL_BRIDGE_ENV_PATH`]",
					"You can specify the path manually with a cli argument: [`--bridge-state-path`]",
					"You can file an issue to get us to auto-detect the best path for your OS.",
				]),
			);
			return;
		}
		let fsemul_path = read_env_path.as_ref().expect("impossible");

		match FSEmulConfig::load_explicit_path(fsemul_path.clone()).await {
			Ok(state) => {
				_ = write_lock.insert(state);
			}
			Err(cause) => {
				error!(
					id = "bridgectl::argv::cannot_load_fsemul_configuration",
					?cause,
					fsemul_path = %fsemul_path.display(),
					"failed to load fsemul configuration file",
				);
			}
		}
	}
}

async fn try_to_load_host_file_system() {
	let read_env_path = CAFE_DATA_PATH.read().await;

	_ = HOST_FILE_SYSTEM.get_or_init(|| {
		if !read_env_path.is_some() {
			error!(
				id = "bridgectl::argv::host_filesystem_required",
				cause = "Could not load the cafe root directory, to serve a host filesystem out of",
				suggestions = valuable(&[
					"You can specify the path manually with an environment variable: [`CAFE_ROOT`]",
					"You can specify the path manually with a cli argument: [`--cafe-path`]",
					"You can file an issue to get us to auto-detect the best path for your OS.",
				]),
			);
			std::process::exit(ARGV_NO_CAFE_ROOT);
		}

		let host_fs_path = read_env_path.as_ref().expect("impossible");

		match futures::executor::block_on(HostFilesystem::from_cafe_dir(Some(host_fs_path.clone())))
		{
			Ok(mut state) => {
				if FORCE_UNIQUE_FDS.load(Ordering::SeqCst) {
					// This is guaranteed to work, the host filesystem _just_ created.
					std::mem::drop(state.force_unique_fds());
				}
				state
			}
			Err(cause) => {
				error!(
					id = "bridgectl::argv::cannot_load_host_file_system",
					?cause,
					host_fs_path = %host_fs_path.display(),
					"failed to load cafe root directory",
				);

				std::process::exit(HOST_FILESYSTEM_INIT_FAILURE);
			}
		}
	});
}

/// Just validate that the bridge configuration is loaded, before attempting
/// to interact with it.
///
/// This should be called AFTER `initialize_host_bridge` has been called,
/// otherwise it's possible we will exit because we think there is no path
/// to the bridge configuration file. Or we might be reading the wrong bridge
/// configuration file.
async fn validate_fsemul_config_is_populated() {
	let read_lock = FSEMUL_CONFIG.read().await;
	let exists = read_lock.is_some();
	std::mem::drop(read_lock);

	if !exists {
		let mut write_lock = FSEMUL_CONFIG.write().await;
		// It's possible while we were waiting to acquire the exclusive write lock
		// that someone else populated the value. If so, let's just return.
		if write_lock.is_some() {
			return;
		}

		let read_env_path = FSEMUL_CONFIG_PATH.read().await;
		if !read_env_path.is_some() {
			error!(
				id = "bridgectl::argv::fsemul_path_required",
				cause = "Could not find the fsemul path aka `fsemul.ini`",
				help = valuable(&[
					"You can specify the path manually with an environment variable: [`BRIDGECTL_FSEMUL_PATH`]",
					"You can specify the path manually with a cli argument: [`--fsemul-config-path`]",
					"You can file an issue to get us to auto-detect the best path for your OS.",
				]),
				"Failed to find FSEmul path!",
			);

			std::process::exit(ARGV_NO_FSEMUL_PATH);
		}
		let fsemul_config_path = read_env_path.as_ref().expect("impossible");

		match FSEmulConfig::load_explicit_path(fsemul_config_path.clone()).await {
			Ok(state) => {
				_ = write_lock.insert(state);
			}
			Err(cause) => {
				error!(
					id = "bridgectl::argv::cannot_load_fsemul_config",
					?cause,
					fsemul_config_path = %fsemul_config_path.display(),
					"failed to load fsemul configuration file",
				);

				std::process::exit(ARGV_FSEMUL_LOAD_FAILURE);
			}
		}
	}
}

/// Try loading the default Cafe SDK Path.
async fn validate_host_file_system_is_populated() {
	let read_env_path = CAFE_DATA_PATH.read().await;
	_ = HOST_FILE_SYSTEM.get_or_init(|| {
		if !read_env_path.is_some() {
			error!(
				id = "bridgectl::argv::cafe_root_path_required",
				help = valuable(&[
					"You can specify the path manually with an environment variable: [`CAFE_ROOT`]",
					"You can specify the path manually with a cli argument: [`--cafe-dir`]",
					"You can file an issue to get us to auto-detect the best path for your OS.",
				]),
				"Could not find the cafe path for the root filesystem",
			);

			std::process::exit(ARGV_NO_CAFE_ROOT);
		}
		let cafe_root_path = read_env_path.as_ref().expect("impossible");

		match futures::executor::block_on(HostFilesystem::from_cafe_dir(Some(
			cafe_root_path.clone(),
		))) {
			Ok(mut state) => {
				if FORCE_UNIQUE_FDS.load(Ordering::SeqCst) {
					// This is guaranteed to work, the host filesystem _just_ created.
					std::mem::drop(state.force_unique_fds());
				}
				state
			}
			Err(cause) => {
				error!(
					id = "bridgectl::argv::cannot_load_host_filesystem",
					?cause,
					cafe_root_path = %cafe_root_path.display(),
					"failed to load host filesystem path",
				);

				std::process::exit(ARGV_CAFE_ROOT_LOAD_FAILURE);
			}
		}
	});
}
