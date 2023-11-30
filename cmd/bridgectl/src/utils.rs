//! Utility functions that don't have one place that they should live.

use crate::{
	exit_codes::{CANT_FIND_BRIDGE_STATE_PATH, CANT_LOAD_BRIDGE_STATE},
	knobs::env::BRIDGE_HOST_STATE_PATH,
};
use cat_dev::BridgeHostState;
use miette::{miette, Report};
use std::path::PathBuf;
use tracing::{error, field::valuable};

/// Add context to a specific error, where you can have like a list of
/// suggestions.
///
/// NOTE: we cannot reassign a reports severity, so your last items severity
///       is where the real severity gets taken.
pub fn add_context_to(
	original_error: Report,
	suggestions: impl DoubleEndedIterator<Item = Report>,
) -> Report {
	let mut latest_error: Option<Report> = None;

	for suggestion in suggestions.rev() {
		if let Some(last_error) = latest_error {
			latest_error = Some(last_error.wrap_err(suggestion));
		} else {
			latest_error = Some(suggestion);
		}
	}

	if let Some(latest) = latest_error {
		latest.wrap_err(original_error)
	} else {
		original_error
	}
}

/// Get the host state from a particular path.
///
/// ## Panics
///
/// If we run into an error calling [`BridgeHostState::load_explicit_path`].
pub async fn bridge_state_from_path(host_state_path: PathBuf, use_json: bool) -> BridgeHostState {
	match BridgeHostState::load_explicit_path(host_state_path.clone()).await {
		Ok(state) => state,
		Err(cause) => {
			if use_json {
				error!(
					id = "bridgectl::cli::cannot_load_host_state",
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

			std::process::exit(CANT_LOAD_BRIDGE_STATE);
		}
	}
}

/// Get the bridge state path to use for initializing a bridge state.
///
/// ## Panics
///
/// If we cannot find a particular bridge state path.
pub(crate) fn get_bridge_state_path(cli_arg: &Option<PathBuf>, use_json: bool) -> PathBuf {
	let mut path_buf = cli_arg.clone();
	if path_buf.is_none() {
		path_buf = BRIDGE_HOST_STATE_PATH.clone();
	}
	if path_buf.is_none() {
		path_buf = BridgeHostState::get_default_host_path();
	}

	let Some(hsp) = path_buf else {
		if use_json {
			error!(
				id = "bridgectl::cli::bridge_state_path_required",
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
						miette!(
							help = format!("`bridge_env.ini` locations specified for this invocation: Cli Argument: {cli_arg:?} / ENV: {:?} / System Default: {:?}", BRIDGE_HOST_STATE_PATH.as_ref(), BridgeHostState::get_default_host_path()),
							"You can file an issue with the project to choose a default directory for your OS.",
						),
					].into_iter(),
				),
			);
		}

		std::process::exit(CANT_FIND_BRIDGE_STATE_PATH);
	};

	hsp
}
