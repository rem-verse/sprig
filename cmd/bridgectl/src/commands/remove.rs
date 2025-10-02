//! Handle the `rm`, or `remove` command to remove a bridge.
//!
//! Specifically remove a bridge from your host state file aka your `bridge_env.ini`.

use crate::{
	commands::argv_helpers::{get_targeted_bridge_name, lease_bridge_config_mut},
	exit_codes::{REMOVE_BRIDGE_DOESNT_EXIST, REMOVE_COULD_NOT_SAVE_TO_DISK},
};
use tracing::{error, info};

/// Handle the removal of a bridge.
pub async fn handle_remove_bridge() {
	let name = get_targeted_bridge_name().await;
	let mut host_state = lease_bridge_config_mut().await;

	if let Some((_potential_ip, is_default)) = host_state.get_bridge(&name) {
		if is_default {
			host_state.remove_default_bridge();
			info!(
				id = "bridgectl::rm::removed_default",
				"The bridge you're removing is your default bridge, so we've unset the default bridge!",
			);
		}
	} else {
		error!(
			id = "bridgectl::rm::bridge_doesnt_exist",
			bridge.name = %name,
			host_state.path = %host_state.get_path().display(),
			"cannot remove a bridge that does not exist",
		);

		std::process::exit(REMOVE_BRIDGE_DOESNT_EXIST);
	}
	host_state.remove_bridge(&name);

	if let Err(cause) = host_state.write_to_disk().await {
		error!(
			id = "bridgectl::rm::could_not_save_to_disk",
			bridge.name = %name,
			host_state.path = %host_state.get_path().display(),
			?cause,
			"could not save changed to disk",
		);

		std::process::exit(REMOVE_COULD_NOT_SAVE_TO_DISK);
	}

	info!(
		id = "bridgectl::rm::success",
		name = name,
		"Successfully removed bridge!"
	);
}
