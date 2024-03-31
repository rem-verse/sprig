//! Common wrapper around the bridge configuration aka the [`BridgeHostState`].
//!
//! Some commands may need to access the configuration file for bridges, this
//! could be for doing things like looking up the default bridge, this could be
//! for setting the default bridge, and things like that. We ideally would only
//! ever open the file once, and read it once. In order to achieve that goal we
//! have these series of functions which wrap around a static safely.

use crate::{
	exit_codes::{ARGV_BRIDGE_STATE_LOAD_FAILURE, ARGV_NO_BRIDGE_STATE_PATH},
	knobs::{cli::BridgeConfigurationFlags, env::BRIDGE_HOST_STATE_PATH},
	utils::add_context_to,
	SHOULD_LOG_JSON,
};
use cat_dev::mion::BridgeHostState;
use miette::miette;
use std::path::PathBuf;
use tokio::sync::{RwLock, RwLockMappedWriteGuard, RwLockReadGuard, RwLockWriteGuard};
use tracing::{error, field::valuable, info};

static BRIDGE_HOST_ENV: RwLock<Option<BridgeHostState>> = RwLock::const_new(None);
static BRIDGE_ENV_PATH: RwLock<Option<PathBuf>> = RwLock::const_new(None);

/// Initialize all of the stuff necessary for fetching from the bridge
/// configuration file.
///
/// This just sets everything up, it doesn't actually open the bridge
/// configuration file, until someone actually requests it for the first time.
pub async fn initialize_host_bridge(bridge_config_flags: BridgeConfigurationFlags) {
	let system_default_path = BridgeHostState::get_default_host_path();
	if system_default_path.is_none() {
		if SHOULD_LOG_JSON() {
			info!(
				id = "bridgectl::argv::no_default_host_state_path",
				"looks like your OS doesn't have a default host state path, please file an issue to support your OS better",
			);
		} else {
			info!("Hey! It looks like we don't have a default path configured for the bridge configuration file. This may mean certain features like setting a default bridge won't work! You can always manually specify a manual place to store the file with `--bridge-state-path`, but we'd really appreciate if you filed an issue to support your OS better!");
		}
	}

	if let Some(cli_arg) = bridge_config_flags.bridge_state_path() {
		let mut locked_env_path = BRIDGE_ENV_PATH.write().await;
		_ = locked_env_path.insert(cli_arg.clone());
	} else if let Some(env_arg) = BRIDGE_HOST_STATE_PATH.as_ref() {
		let mut locked_env_path = BRIDGE_ENV_PATH.write().await;
		_ = locked_env_path.insert(env_arg.clone());
	} else if let Some(default_path) = system_default_path {
		let mut locked_env_path = BRIDGE_ENV_PATH.write().await;
		_ = locked_env_path.insert(default_path);
	}
}

/// Optionally lease the bridge configuration if it was able to be parsed, but you don't
/// want to exit if it's not present.
pub async fn lease_bridge_config_optionally<'lf>() -> Option<RwLockReadGuard<'lf, BridgeHostState>>
{
	try_to_load_bridge_config().await;

	let read_lock = BRIDGE_HOST_ENV.read().await;
	if read_lock.is_some() {
		Some(RwLockReadGuard::map(read_lock, |inner_guard| {
			inner_guard.as_ref().expect("impossible")
		}))
	} else {
		None
	}
}

/// Get a non-mutable reference to the current bridge configuration.
///
/// This will exit the program if for some reason we can't load the bridge
/// configuration file from the disk.
pub async fn lease_bridge_config<'lf>() -> RwLockReadGuard<'lf, BridgeHostState> {
	validate_bridge_config_is_populated().await;

	RwLockReadGuard::map(BRIDGE_HOST_ENV.read().await, |inner_guard| {
		inner_guard.as_ref().expect("impossible")
	})
}

/// Get a mutable reference to the current bridge configuration.
///
/// This will exit the program if for some reason we can't load the bridge
/// configuration file from the disk.
pub async fn lease_bridge_config_mut<'lf>() -> RwLockMappedWriteGuard<'lf, BridgeHostState> {
	validate_bridge_config_is_populated().await;

	RwLockWriteGuard::map(BRIDGE_HOST_ENV.write().await, |inner_guard| {
		inner_guard.as_mut().expect("impossible")
	})
}

async fn try_to_load_bridge_config() {
	let read_lock = BRIDGE_HOST_ENV.read().await;
	let exists = read_lock.is_some();
	std::mem::drop(read_lock);

	if !exists {
		let mut write_lock = BRIDGE_HOST_ENV.write().await;
		// It's possible while we were waiting to acquire the exclusive write lock
		// that someone else populated the value. If so, let's just return.
		if write_lock.is_some() {
			return;
		}

		let read_env_path = BRIDGE_ENV_PATH.read().await;
		if !read_env_path.is_some() {
			if SHOULD_LOG_JSON() {
				error!(
					id = "bridgectl::argv::bridge_state_path_required",
					cause = "Could not find the bridge state path aka `bridge_env.ini`",
					suggestions = valuable(&[
						"You can specify the path manually with an environment variable: [`BRIDGECTL_BRIDGE_ENV_PATH`]",
						"You can specify the path manually with a cli argument: [`--bridge-state-path`]",
						"You can file an issue to get us to auto-detect the best path for your OS.",
					]),
				);
			} else {
				error!(
					"\n{:?}",
					add_context_to(
						miette!("Could not find the path to store the bridge-host state file!"),
						[
							miette!("You can specify the path to the `bridge_env.ini` file with the environment variable `BRIDGECTL_BRIDGE_ENV_PATH`"),
							miette!("You can specify the path to the `bridge_env.ini` file with the cli argument `--bridge-state-path`"),
							miette!("You can file an issue with the project to choose a default directory for your OS."),
						].into_iter(),
					),
				);
			}
			return;
		}
		let host_state_path = read_env_path.as_ref().expect("impossible");

		match BridgeHostState::load_explicit_path(host_state_path.clone()).await {
			Ok(state) => {
				_ = write_lock.insert(state);
			}
			Err(cause) => {
				if SHOULD_LOG_JSON() {
					error!(
						id = "bridgectl::argv::cannot_load_host_state",
						?cause,
						host_state_path = %host_state_path.display(),
						"failed to load host state file",
					);
				} else {
					error!(
						"\n{:?}",
						miette!(
							help = format!(
								"Host State File is located at: {}",
								host_state_path.display()
							),
							"Cannot load host state file!",
						)
						.wrap_err(cause),
					);
				}
			}
		}
	}
}

/// Just validate that the bridge configuration is loaded, before attempting
/// to interact with it.
///
/// This should be called AFTER `initialize_host_bridge` has been called,
/// otherwise it's possible we will exit because we think there is no path
/// to the bridge configuration file. Or we might be reading the wrong bridge
/// configuration file.
async fn validate_bridge_config_is_populated() {
	let read_lock = BRIDGE_HOST_ENV.read().await;
	let exists = read_lock.is_some();
	std::mem::drop(read_lock);

	if !exists {
		let mut write_lock = BRIDGE_HOST_ENV.write().await;
		// It's possible while we were waiting to acquire the exclusive write lock
		// that someone else populated the value. If so, let's just return.
		if write_lock.is_some() {
			return;
		}

		let read_env_path = BRIDGE_ENV_PATH.read().await;
		if !read_env_path.is_some() {
			if SHOULD_LOG_JSON() {
				error!(
					id = "bridgectl::argv::bridge_state_path_required",
					cause = "Could not find the bridge state path aka `bridge_env.ini`",
					suggestions = valuable(&[
						"You can specify the path manually with an environment variable: [`BRIDGECTL_BRIDGE_ENV_PATH`]",
						"You can specify the path manually with a cli argument: [`--bridge-state-path`]",
						"You can file an issue to get us to auto-detect the best path for your OS.",
					]),
				);
			} else {
				error!(
					"\n{:?}",
					add_context_to(
						miette!("Could not find the path to store the bridge-host state file!"),
						[
							miette!("You can specify the path to the `bridge_env.ini` file with the environment variable `BRIDGECTL_BRIDGE_ENV_PATH`"),
							miette!("You can specify the path to the `bridge_env.ini` file with the cli argument `--bridge-state-path`"),
							miette!("You can file an issue with the project to choose a default directory for your OS."),
						].into_iter(),
					),
				);
			}

			std::process::exit(ARGV_NO_BRIDGE_STATE_PATH);
		}
		let host_state_path = read_env_path.as_ref().expect("impossible");

		match BridgeHostState::load_explicit_path(host_state_path.clone()).await {
			Ok(state) => {
				_ = write_lock.insert(state);
			}
			Err(cause) => {
				if SHOULD_LOG_JSON() {
					error!(
						id = "bridgectl::argv::cannot_load_host_state",
						?cause,
						host_state_path = %host_state_path.display(),
						"failed to load host state file",
					);
				} else {
					error!(
						"\n{:?}",
						miette!(
							help = format!(
								"Host State File is located at: {}",
								host_state_path.display()
							),
							"Cannot load host state file!",
						)
						.wrap_err(cause),
					);
				}

				std::process::exit(ARGV_BRIDGE_STATE_LOAD_FAILURE);
			}
		}
	}
}
