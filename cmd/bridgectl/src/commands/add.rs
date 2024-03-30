//! Handles the `add`, or `update` command for `bridgectl`.

use crate::{
	commands::argv_helpers::{
		get_targeted_bridge_ip, get_targeted_bridge_name, lease_bridge_config_mut,
	},
	exit_codes::{ADD_COULD_NOT_SAVE_TO_DISK, ADD_COULD_NOT_UPSERT},
	utils::add_context_to,
	SHOULD_LOG_JSON,
};
use miette::miette;
use tracing::{error, info};

/// Handle adding a bridge, or updating a bridge.
pub async fn handle_add_or_update(set_default: bool) {
	let bridge_ip = get_targeted_bridge_ip().await;
	let bridge_name = get_targeted_bridge_name().await;
	let mut host_state = lease_bridge_config_mut().await;

	if let Err(cause) = host_state.upsert_bridge(&bridge_name, bridge_ip) {
		if SHOULD_LOG_JSON() {
			error!(
				id = "bridgectl::add::upsert_failed",
				?cause,
				%bridge_name,
				%bridge_ip,
				"Please ensure bridge name we're adding is a valid bridge name.",
			);
		} else {
			error!(
				"\n{:?}",
				add_context_to(
					miette!(
						"Could not add bridge to host state file, bridge name must not be valid."
					),
					[
						cause.into(),
						miette!(
							help = format!("Arguments were: Bridge Name: {bridge_name} / Bridge IP: {bridge_ip}"),
							"Bridge Names must be ASCII, and between 1-255 characters long.",
						),
					]
					.into_iter(),
				),
			);
		}

		std::process::exit(ADD_COULD_NOT_UPSERT);
	}

	if set_default {
		// Guaranteed not to fail, because upsert succeeded above.
		_ = host_state.set_default_bridge(&bridge_name);
	}

	if let Err(cause) = host_state.write_to_disk().await {
		if SHOULD_LOG_JSON() {
			error!(
				id = "bridgectl::add::write_to_disk_failure",
				?cause,
				path = %host_state.get_path().display(),
			);
		} else {
			error!(
				"\n{:?}",
				miette!(
					help = format!("Host state path is: {}", host_state.get_path().display()),
					"Could not write the new host state file to disk! Change is not persisted!"
				)
				.wrap_err(cause),
			);
		}

		std::process::exit(ADD_COULD_NOT_SAVE_TO_DISK);
	}

	info!(
		id = "bridgectl::add::success",
		%bridge_name,
		%bridge_ip,
		"Successfully added a bridge to your host state file!{}",
		if set_default { " And successfully set it as your default bridge." } else { "" }
	);
}
