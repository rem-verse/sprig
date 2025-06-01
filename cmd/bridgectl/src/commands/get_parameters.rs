use crate::{
	SHOULD_LOG_JSON,
	commands::argv_helpers::get_targeted_bridge_ip,
	exit_codes::{GET_PARAMS_FAILED_TO_GET_PARAMS, GET_PARAMS_NO_PARAMETERS_SPECIFIED},
	utils::add_context_to,
};
use cat_dev::mion::{parameter::get_parameters, proto::parameter::DumpedMionParameters};
use miette::miette;
use std::net::Ipv4Addr;
use tracing::{debug, error, field::valuable, info};

/// Actual command handler for the `get-parameters`, or `gp` command.
pub async fn handle_get_parameters(
	bridge_or_params_arguments: Option<String>,
	only_params_arguments: Option<String>,
	parameter_space_port: Option<u16>,
	used_first_arg: bool,
) {
	if let Some(params) = only_params_arguments {
		let bridge_ip = get_targeted_bridge_ip().await;
		print_parameters(
			&params,
			&fetch_parameters(bridge_ip, parameter_space_port).await,
		);
		return;
	} else if let Some(params) = bridge_or_params_arguments {
		if !used_first_arg {
			let bridge_ip = get_targeted_bridge_ip().await;
			print_parameters(
				&params,
				&fetch_parameters(bridge_ip, parameter_space_port).await,
			);
			return;
		}
	}

	if SHOULD_LOG_JSON() {
		error!(
			id = "bridgectl::get_params::no_params",
			suggestions = valuable(&[
				"You can run `bridgectl get-params <bridge> <params>`, `bridgectl gp --default <params>`, etc.",
				"If running in a mochiato/cafe/cafex environment you can run: `bridgectl get-params <params>`.",
				"You can run `bridgectl get-params --help` to get more information.",
			]),
			"No parameter arguments passed to `bridgectl get-params`, but we need a list of parameters to fetch!",
		);
	} else {
		error!(
			"\n{:?}",
			add_context_to(
				miette!("No parameter arguments passed to `bridgectl get-params`, but we need a list of parameters to fetch"),
				[
					miette!("You can run `bridgectl get-params <bridge> <params>`, `bridgectl gp --default <params>`, etc."),
					miette!("If running in a mochiato/cafe/cafex environment you can run: `bridgectl get-params <params>`."),
					miette!("You can run `bridgectl get-params --help` to get more information on how to use this command."),
				].into_iter(),
			),
		);
	}

	std::process::exit(GET_PARAMS_NO_PARAMETERS_SPECIFIED);
}

fn print_parameters(parameter_filters: &str, parameters: &DumpedMionParameters) {
	for filter in parameter_filters.split(',') {
		if filter.is_empty() {
			if SHOULD_LOG_JSON() {
				debug!(
					id = "bridgectl::get_parameters::empty_parameter_filter",
					line = "Filter in parameter filters was empty, continuing.",
				);
			} else {
				debug!("Filter in parameter filters was empty, continuing.");
			}
			continue;
		}

		if let Ok(parameter_value) = parameters.get_parameter_by_name(filter) {
			if SHOULD_LOG_JSON() {
				info!(
					id = "bridgectl::get_parameters::parameter_found",
					line="Found parameter!",
					parameter.name=%filter,
					parameter.value=%parameter_value,
				);
			} else {
				info!(parameter.name=%filter, parameter.value=%parameter_value, "Found your parameter!");
			}
		} else if SHOULD_LOG_JSON() {
			error!(
				id = "bridgectl::get_parameters::parameter_not_found",
				parameter.name = %filter,
				line = "Could not find the parameter name, or index (0-511) with the name/index you specified. Please ensure you have typed it correctly.",
			);
		} else {
			error!(
				parameter.name = %filter,
				"Could not find the parameter name, or index (0-511) with the name/index you specified. Please ensure you have typed it correctly."
			);
		}
	}
}

async fn fetch_parameters(bridge_ip: Ipv4Addr, bridge_port: Option<u16>) -> DumpedMionParameters {
	match get_parameters(bridge_ip, bridge_port, None).await {
		Ok(params) => params,
		Err(cause) => {
			if SHOULD_LOG_JSON() {
				error!(
					id = "bridgectl::get_parameters::failed_to_execute_get_parameters",
					?cause,
					help = "We could not send/receive a packet to your MION to ask for it's parameters, please ensure it is running. If it's been running for awhile, it may need a reboot.",
				);
			} else {
				error!(
					"\n{:?}",
					miette!(
						help = "If you leave a MION running for too long it may stop responding to parameter requests.",
						"Could not send/receive a packet to your MION to ask for it's parameters, please ensure the device is running.",
					)
					.wrap_err(cause),
				);
			}
			std::process::exit(GET_PARAMS_FAILED_TO_GET_PARAMS);
		}
	}
}
