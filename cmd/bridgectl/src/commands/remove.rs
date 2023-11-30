//! Handle the `rm`, or `remove` command to remove a bridge.
//!
//! Specifically remove a bridge from your host state file aka your `bridge_env.ini`.

use crate::{
	exit_codes::{
		REMOVE_BRIDGE_DOESNT_EXIST, REMOVE_CONFLICTING_ARGUMENTS, REMOVE_COULD_NOT_SAVE_TO_DISK,
		REMOVE_NO_ARGUMENTS,
	},
	utils::{add_context_to, bridge_state_from_path, get_bridge_state_path},
};
use cat_dev::BridgeHostState;
use miette::miette;
use std::path::PathBuf;
use tracing::{error, field::valuable, info};

/// Handle the removal of a bridge.
pub async fn handle_remove_bridge(
	use_json: bool,
	flag_argument: Option<String>,
	positional_argument: Option<String>,
	bridge_state_path: Option<PathBuf>,
) {
	let argument = match (flag_argument, positional_argument) {
		(Some(flag), Some(positional)) => {
			if flag == positional {
				flag
			} else {
				if use_json {
					error!(
						id = "bridgectl::rm::conflicting_arguments",
						name.flag = flag,
						name.positional = positional,
						"Cannot provide both a flag for name, and a positional argument for name.",
					);
				} else {
					error!(
						"\n{:?}",
						miette!(
              help = format!("Flag Argument: {flag} / Positional Argument: {positional}"),
              "Cannot provide both a flag for name, and a positional argument for name.",
            ),
					);
				}

				std::process::exit(REMOVE_CONFLICTING_ARGUMENTS);
			}
		}
		(None, None) => {
			if use_json {
				error!(
					id = "bridgectl::rm::no_arguments",
					suggestions = valuable(&[
						"You can run `bridgectl rm <name>`, or `bridgectl rm --name <name>`.",
						"You can run `bridgectl rm --help` to get more information.",
					]),
					"No provided arguments to `bridgectl rm`, but we need a name to remove!",
				);
			} else {
				error!(
          "\n{:?}",
          add_context_to(
            miette!("No provided arguments to `bridgectl rm`, but we need a name to remove!"),
            [
              miette!("You can run `bridgectl rm <name>`, or `bridgectl rm --name <name>`."),
              miette!("You can run `bridgectl rm --help` to get more information on how to use this command."),
            ].into_iter(),
          ),
        );
			}

			std::process::exit(REMOVE_NO_ARGUMENTS);
		}
		(Some(value), None) | (None, Some(value)) => value,
	};

	let state = bridge_state_from_path(
		get_bridge_state_path(&bridge_state_path, use_json),
		use_json,
	)
	.await;
	remove_bridge_from_state(use_json, state, argument).await;
}

async fn remove_bridge_from_state(use_json: bool, mut host_state: BridgeHostState, name: String) {
	if let Some((_potential_ip, is_default)) = host_state.get_bridge(&name) {
		if is_default {
			host_state.remove_default_bridge();
			info!(id = "bridgectl::rm::removed_default", "The bridge you're removing is your default bridge, so we've unset the default bridge!");
		}
	} else {
		if use_json {
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
            [
              miette!(
                "Please ensure the bridge name {name} isn't mispelled and is present in the state file at: {}",
                host_state.get_path().display(),
              ),
            ].into_iter(),
          ),
        );
		}

		std::process::exit(REMOVE_BRIDGE_DOESNT_EXIST);
	}
	host_state.remove_bridge(&name);

	if let Err(cause) = host_state.write_to_disk().await {
		if use_json {
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
