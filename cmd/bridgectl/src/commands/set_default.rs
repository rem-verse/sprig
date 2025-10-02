//! Handle setting the default bridge to use for your system.
//!
//! This is based off of your `bridge_env.ini` file that is present on your
//! system.

use crate::{
	commands::argv_helpers::{get_targeted_bridge_name, lease_bridge_config_mut},
	exit_codes::{SET_DEFAULT_BRIDGE_DOESNT_EXIST, SET_DEFAULT_COULD_NOT_SAVE_TO_DISK},
};
use tracing::{error, info};

/// Handle the set default bridge command.
pub async fn handle_set_default_bridge() {
	let name = get_targeted_bridge_name().await;
	let mut host_state = lease_bridge_config_mut().await;

	if host_state.get_bridge(&name).is_none() {
		error!(
			id = "bridgectl::set_default::bridge_doesnt_exist",
			bridge.name = %name,
			host_state.path = %host_state.get_path().display(),
			"cannot set a bridge as the default that does not exist",
		);

		std::process::exit(SET_DEFAULT_BRIDGE_DOESNT_EXIST);
	}

	let old_default = host_state.get_default_bridge().map(|(name, _opt_ip)| name);
	// Bridge exists -- guaranteed to be safe to add as a default.
	_ = host_state.set_default_bridge(&name);

	if let Err(cause) = host_state.write_to_disk().await {
		error!(
			id = "bridgectl::set_default::could_not_save_to_disk",
			bridge.name = %name,
			host_state.path = %host_state.get_path().display(),
			?cause,
			"could not save changed to disk",
		);

		std::process::exit(SET_DEFAULT_COULD_NOT_SAVE_TO_DISK);
	}

	info!(
		id="bridgectl::set_default::success",
		default.old = ?old_default,
		default.new = name,
		"Set your bridge as the default!"
	);
}
