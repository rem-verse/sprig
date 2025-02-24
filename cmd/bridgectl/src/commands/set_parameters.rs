use crate::{
	SHOULD_LOG_JSON,
	commands::argv_helpers::{get_byte_value, get_targeted_bridge_ip},
	exit_codes::{
		SET_PARAMS_FAILED_TO_SET_PARAMS, SET_PARAMS_INVALID_PARAMETER_SET_STRING,
		SET_PARAMS_INVALID_PARAMETER_VALUE, SET_PARAMS_NO_PARAMETERS_SPECIFIED,
	},
	utils::add_context_to,
};
use cat_dev::mion::{
	parameter::set_parameters,
	proto::parameter::well_known::{ParameterLocationSpecification, validate_value_at_index},
};
use miette::miette;
use std::net::Ipv4Addr;
use tracing::{error, field::valuable, info, warn};

/// Actual command handler for the `set-parameters`, or `sp` command.
pub async fn handle_set_parameters(
	bridge_or_params_arguments: Option<String>,
	only_params_arguments: Option<String>,
	parameter_space_port: Option<u16>,
	used_first_arg: bool,
) {
	if let Some(params) = only_params_arguments {
		let param_filters = parse_parameters_to_set_list(&params);
		let bridge_ip = get_targeted_bridge_ip().await;
		do_set_parameters(bridge_ip, parameter_space_port, param_filters).await;
	} else if let Some(params) = bridge_or_params_arguments {
		if !used_first_arg {
			let param_filters = parse_parameters_to_set_list(&params);
			let bridge_ip = get_targeted_bridge_ip().await;
			do_set_parameters(bridge_ip, parameter_space_port, param_filters).await;
		}
	}

	if SHOULD_LOG_JSON() {
		error!(
			id = "bridgectl::set_parameters::no_params",
			suggestions = valuable(&[
				"You can run `bridgectl set-params <bridge> <params>`, `bridgectl sp --default <params>`, etc.",
				"If running in a mochiato/cafe/cafex environment you can run: `bridgectl set-params <params>`.",
				"You can run `bridgectl set-params --help` to get more information.",
			]),
			"No parameter arguments passed to `bridgectl set-params`, but we need a list of parameters to fetch!",
		);
	} else {
		error!(
			"\n{:?}",
			add_context_to(
				miette!("No parameter arguments passed to `bridgectl set-params`, but we need a list of parameters to fetch"),
				[
					miette!("You can run `bridgectl set-params <bridge> <params>`, `bridgectl sp --default <params>`, etc."),
					miette!("If running in a mochiato/cafe/cafex environment you can run: `bridgectl set-params <params>`."),
					miette!("You can run `bridgectl set-params --help` to get more information on how to use this command."),
				].into_iter(),
			),
		);
	}
	std::process::exit(SET_PARAMS_NO_PARAMETERS_SPECIFIED);
}

async fn do_set_parameters(
	ip: Ipv4Addr,
	parameter_space_port: Option<u16>,
	parameters_to_set: Vec<(ParameterLocationSpecification, u8)>,
) {
	match set_parameters(
		parameters_to_set.into_iter(),
		ip,
		parameter_space_port,
		None,
	)
	.await
	{
		Ok(_) => {
			info!("Successfully set your parameters!");
		}
		Err(cause) => {
			if SHOULD_LOG_JSON() {
				error!(
					id = "bridgectl::set_parameters::failed_to_execute_set_parameters",
					?cause,
					help = "We could not send/receive a packet to your MION to set it's parameters, please ensure it is running. If it's been running for awhile, it may need a reboot.",
				);
			} else {
				error!(
					"\n{:?}",
					miette!(
						help = "If you leave a MION running for too long it may stop responding to parameter requests.",
						"Could not send/receive a packet to your MION to set it's parameters, please ensure the device is running.",
					)
					.wrap_err(cause),
				);
			}

			std::process::exit(SET_PARAMS_FAILED_TO_SET_PARAMS);
		}
	}
}

fn parse_parameters_to_set_list(
	parameters_string: &str,
) -> Vec<(ParameterLocationSpecification, u8)> {
	let mut locations = Vec::new();
	for potential_serialized_specification in parameters_string.split(',') {
		let Some(found_equals_location) = potential_serialized_specification.find('=') else {
			if SHOULD_LOG_JSON() {
				error!(
					id = "bridgectl::set_parameters::no_equals_sign",
					parameter = %potential_serialized_specification,
					line = "Parameters in set parameters should be in the format `(name or idx)=(value)`, but noe quals sign was found!",
				);
			} else {
				error!(
					parameter = %potential_serialized_specification,
					"Parameters for set-parameters should be in the format `(name or idx)=(value)` e.g. `major=2`, or `3=5`!",
				);
			}

			std::process::exit(SET_PARAMS_INVALID_PARAMETER_SET_STRING);
		};

		let (index_or_name, mut str_value) =
			potential_serialized_specification.split_at(found_equals_location);
		// Guaranteed to have at least one value, because the equal signs is there.
		str_value = &str_value[1..];

		let Ok(specification) = ParameterLocationSpecification::try_from(index_or_name) else {
			if SHOULD_LOG_JSON() {
				error!(
					id = "bridgectl::set_parameters::bad_parameter_name",
					parameter.name = %index_or_name,
					line = "Parameter name wasn't known, or index wasn't within range of (0-511 inclusive).",
				);
			} else {
				error!(
					parameter.name = %index_or_name,
					"Parameter name was not known, or it was an index who wasn't within the range of 0-511."
				);
			}

			std::process::exit(SET_PARAMS_INVALID_PARAMETER_SET_STRING);
		};
		let Ok(value_as_byte) = get_byte_value(str_value) else {
			if SHOULD_LOG_JSON() {
				error!(
					id = "bridgectl::set_parameters::bad_parameter_value",
					parameter.name = %index_or_name,
					parameter.value = %str_value,
					line = "Parameters can only be set to a byte value (0-255 inclusive).",
				);
			} else {
				error!(
					parameter.name = %index_or_name,
					parameter.value = %str_value,
					line = "Parameters can only be set to a byte value (0-255 inclusive), your value could not be parsed as a number in that range.",
				);
			}

			std::process::exit(SET_PARAMS_INVALID_PARAMETER_VALUE);
		};

		if !validate_value_at_index(&specification, value_as_byte) {
			if SHOULD_LOG_JSON() {
				warn!(
					id = "bridgectl::set_parameters::invalid_parameter_value",
					parameter.name = ?specification,
					parameter.value = value_as_byte,
					"this value is not valid for the location",
				);
			} else {
				warn!(
					parameter.name = ?specification,
					parameter.value = value_as_byte,
					"This parameter value is not valid, and may run into various errors.",
				);
			}
		}

		locations.push((specification, value_as_byte));
	}
	locations
}
