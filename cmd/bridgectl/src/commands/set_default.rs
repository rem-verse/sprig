//! Handle setting the default bridge to use for your system.
//!
//! This is based off of your `bridge_env.ini` file that is present on your
//! system.

use crate::{
	exit_codes::{
		SET_DEFAULT_BRIDGE_DOESNT_EXIST, SET_DEFAULT_CONFLICTING_ARGUMENTS,
		SET_DEFAULT_COULD_NOT_SAVE_TO_DISK, SET_DEFAULT_NO_ARGUMENTS,
	},
	utils::{add_context_to, bridge_state_from_path, get_bridge_state_path},
};
use cat_dev::BridgeHostState;
use miette::miette;
use std::path::PathBuf;
use tracing::{error, field::valuable, info};

/// Handle the set default bridge command.
pub async fn handle_set_default_bridge(
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
						id = "bridgectl::set_default::conflicting_arguments",
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

				std::process::exit(SET_DEFAULT_CONFLICTING_ARGUMENTS);
			}
		}
		(None, None) => {
			if use_json {
				error!(
					id = "bridgectl::set_default::no_arguments",
					suggestions = valuable(&[
						"You can run `bridgectl set-default <name>`, or `bridgectl set-default --name <name>`.",
						"You can run `bridgectl set-default --help` to get more information.",
					]),
					"No provided arguments to `bridgectl set-default`, but we need a name of a bridge to make the default!",
				);
			} else {
				error!(
          "\n{:?}",
          add_context_to(
            miette!("No provided arguments to `bridgectl set-default`, but we need a name of a bridge to make the default"),
            [
              miette!("You can run `bridgectl set-default <name>`, or `bridgectl set-default --name <name>`."),
              miette!("You can run `bridgectl set-default --help` to get more information on how to use this command."),
            ].into_iter(),
          ),
        );
			}

			std::process::exit(SET_DEFAULT_NO_ARGUMENTS);
		}
		(Some(value), None) | (None, Some(value)) => value,
	};

	let state = bridge_state_from_path(
		get_bridge_state_path(&bridge_state_path, use_json),
		use_json,
	)
	.await;
	set_default_bridge(use_json, state, argument).await;
}

async fn set_default_bridge(use_json: bool, mut host_state: BridgeHostState, name: String) {
	if host_state.get_bridge(&name).is_none() {
		if use_json {
			error!(
			  id = "bridgectl::set_default::bridge_doesnt_exist",
			  bridge.name = %name,
			  host_state.path = %host_state.get_path().display(),
			  "cannot set a bridge as the default that does not exist",
			);
		} else {
			error!(
          "\n{:?}",
          add_context_to(
            miette!("cannot set a bridge as the default that does not exist"),
            [
              miette!(
                "Please ensure the bridge name {name} isn't mispelled and is present in the state file at: {}",
                host_state.get_path().display(),
              ),
            ].into_iter(),
          ),
        );
		}

		std::process::exit(SET_DEFAULT_BRIDGE_DOESNT_EXIST);
	}

	let old_default = host_state.get_default_bridge().map(|(name, _opt_ip)| name);
	// Bridge exists -- guaranteed to be safe to add as a default.
	_ = host_state.set_default_bridge(&name);

	if let Err(cause) = host_state.write_to_disk().await {
		if use_json {
			error!(
			  id = "bridgectl::set_default::could_not_save_to_disk",
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
						"While trying to set bridge named {name} as the default bridge for: {}",
						host_state.get_path().display()
					),
					"could not save changes directly to disk",
				)
				.wrap_err(cause),
			);
		}

		std::process::exit(SET_DEFAULT_COULD_NOT_SAVE_TO_DISK);
	}

	info!(
	  id="bridgectl::set_default::success",
	  default.old = ?old_default,
	  default.new = name,
	  "Set your bridge as the default!"
	);
}
