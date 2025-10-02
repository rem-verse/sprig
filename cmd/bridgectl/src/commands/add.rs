//! Handles the `add`, or `update` command for `bridgectl`.

use crate::{
	commands::argv_helpers::{
		get_targeted_bridge_ip, get_targeted_bridge_name, lease_bridge_config_mut,
	},
	exit_codes::{ADD_COULD_NOT_SAVE_TO_DISK, ADD_COULD_NOT_UPSERT},
};
use tracing::{error, info};

/// Handle adding a bridge, or updating a bridge.
pub async fn handle_add_or_update(set_default: bool) {
	let bridge_ip = get_targeted_bridge_ip().await;
	let bridge_name = get_targeted_bridge_name().await;
	let mut host_state = lease_bridge_config_mut().await;

	if let Err(cause) = host_state.upsert_bridge(&bridge_name, bridge_ip) {
		error!(
			id = "bridgectl::add::upsert_failed",
			?cause,
			%bridge_name,
			%bridge_ip,
			help = "Bridge names must be ASCII, and between 1-255 characters long.",
			"Please ensure bridge name we're adding is a valid bridge name.",
		);

		std::process::exit(ADD_COULD_NOT_UPSERT);
	}

	if set_default {
		// Guaranteed not to fail, because upsert succeeded above.
		_ = host_state.set_default_bridge(&bridge_name);
	}

	if let Err(cause) = host_state.write_to_disk().await {
		error!(
			id = "bridgectl::add::write_to_disk_failure",
			?cause,
			path = %host_state.get_path().display(),
		);

		std::process::exit(ADD_COULD_NOT_SAVE_TO_DISK);
	}

	let mut line = "Successfully added a bridge to your host state file!{}".to_owned();
	if set_default {
		line += " And successfully set it as your default bridge.";
	}
	info!(
		id = "bridgectl::add::success",
		%bridge_name,
		%bridge_ip,
		line,
	);
}
