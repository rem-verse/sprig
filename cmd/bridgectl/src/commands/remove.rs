//! Handle the `rm`, or `remove` command to remove a bridge.
//!
//! Specifically remove a bridge from your host state file aka your `bridge_env.ini`.

use crate::{
	SHOULD_LOG_JSON,
	commands::argv_helpers::{get_targeted_bridge_name, lease_bridge_config_mut},
	exit_codes::{REMOVE_BRIDGE_DOESNT_EXIST, REMOVE_COULD_NOT_SAVE_TO_DISK},
	utils::add_context_to,
};
use miette::miette;
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
		if SHOULD_LOG_JSON() {
			error!(
				id = "bridgectl::rm::bridge_doesnt_exist",
				bridge.name = %name,
				host_state.path = %host_state.get_path().display(),
				"cannot remove a bridge that does not exist",
			);
		} else {
			error!(
				"\n{:?}",
				add_context_to(
					miette!("cannot remove a bridge that does not exist"),
					[miette!(
							"Please ensure the bridge name {name} isn't mispelled and is present in the state file at: {}",
							host_state.get_path().display(),
						)]
					.into_iter(),
				),
			);
		}

		std::process::exit(REMOVE_BRIDGE_DOESNT_EXIST);
	}
	host_state.remove_bridge(&name);

	if let Err(cause) = host_state.write_to_disk().await {
		if SHOULD_LOG_JSON() {
			error!(
				id = "bridgectl::rm::could_not_save_to_disk",
				bridge.name = %name,
				host_state.path = %host_state.get_path().display(),
				?cause,
				"could not save changed to disk",
			);
		} else {
			error!(
				"\n{:?}",
				miette!(
					help = format!(
						"While trying to remove bridge named {name} from: {}",
						host_state.get_path().display()
					),
					"could not save changes directly to disk",
				)
				.wrap_err(cause),
			);
		}

		std::process::exit(REMOVE_COULD_NOT_SAVE_TO_DISK);
	}

	info!(
		id = "bridgectl::rm::success",
		name = name,
		"Successfully removed bridge!"
	);
}
