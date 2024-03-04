use crate::{
	commands::argv_helpers::{coalesce_bridge_arguments, get_byte_value, get_default_bridge},
	exit_codes::{
		SET_PARAMS_FAILED_TO_SET_PARAMS, SET_PARAMS_INVALID_PARAMETER_SET_STRING,
		SET_PARAMS_INVALID_PARAMETER_VALUE, SET_PARAMS_NO_AVAILABLE_BRIDGE,
		SET_PARAMS_NO_BRIDGE_FILTERS, SET_PARAMS_NO_PARAMETERS_SPECIFIED,
	},
	knobs::env::{BRIDGE_CURRENT_IP_ADDRESS, BRIDGE_CURRENT_NAME},
	utils::add_context_to,
};
use cat_dev::mion::{
	discovery::{find_mion, MIONFindBy},
	parameter::set_parameters,
	proto::parameter::well_known::ParameterLocationSpecification,
};
use miette::miette;
use std::{net::Ipv4Addr, path::PathBuf, time::Duration};
use tracing::{error, field::valuable, info};

/// Actual command handler for the `set-parameters`, or `sp` command.
#[allow(
	// This is unfortunate that there are a lot, but the command accepts lots of
	// potential parameters.
	//
	// The parameters are also fairly different types, so the chances of screwing
	// up passing them in without noticing is low.
	clippy::too_many_arguments,
)]
pub async fn handle_set_parameters(
	use_json: bool,
	just_fetch_default: bool,
	bridge_flag_arguments: (Option<Ipv4Addr>, Option<String>, Option<String>),
	bridge_or_params_arguments: Option<String>,
	only_params_arguments: Option<String>,
	find_by_args: (Duration, u16),
	parameter_space_port: Option<u16>,
	host_state_path: Option<PathBuf>,
) {
	let had_params_arg = only_params_arguments.is_some();
	let (param_filters, bridge_name_arg) = if let Some(params) = only_params_arguments {
		(params, bridge_or_params_arguments)
	} else if let Some(params) = bridge_or_params_arguments {
		(params, None)
	} else {
		if use_json {
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
	};
	let parameters_to_set = parse_parameters_to_set_list(use_json, &param_filters);

	let bridge_ip = get_a_bridge_ip(
		use_json,
		just_fetch_default,
		bridge_flag_arguments,
		bridge_name_arg,
		had_params_arg,
		find_by_args,
		host_state_path,
	)
	.await;
	do_set_parameters(use_json, bridge_ip, parameter_space_port, parameters_to_set).await;
}

async fn do_set_parameters(
	use_json: bool,
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
			if use_json {
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
	use_json: bool,
	parameters_string: &str,
) -> Vec<(ParameterLocationSpecification, u8)> {
	let mut locations = Vec::new();
	for potential_serialized_specification in parameters_string.split(',') {
		let Some(found_equals_location) = potential_serialized_specification.find('=') else {
			if use_json {
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
			if use_json {
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
			if use_json {
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

		locations.push((specification, value_as_byte));
	}
	locations
}

#[allow(
	// This is barely over, and I don't think it's worth it to lower the count.
	clippy::too_many_lines,
)]
async fn get_a_bridge_ip(
	use_json: bool,
	just_fetch_default: bool,
	bridge_flag_arguments: (Option<Ipv4Addr>, Option<String>, Option<String>),
	bridge_or_params_argument: Option<String>,
	had_params_arg: bool,
	find_by_args: (Duration, u16),
	host_state_path: Option<PathBuf>,
) -> Ipv4Addr {
	if let Some((filter_ip, filter_mac, filter_name)) = coalesce_bridge_arguments(
		use_json,
		just_fetch_default,
		bridge_flag_arguments,
		bridge_or_params_argument,
		had_params_arg,
	) {
		if filter_ip.is_none() && filter_mac.is_none() && filter_name.is_none() {
			if let Some(ip_address) = *BRIDGE_CURRENT_IP_ADDRESS {
				return ip_address;
			} else if let Some(name) = BRIDGE_CURRENT_NAME.as_deref() {
				return get_mochiato_bridge_ip(use_json, name, find_by_args).await;
			} else if use_json {
				error!(
					id = "bridgectl::set_parameters::no_bridge_filters",
					help = "You didn't specify any bridge to set the parameters of!",
				);
				std::process::exit(SET_PARAMS_NO_BRIDGE_FILTERS);
			} else {
				error!(
					"\n{:?}",
					miette!(
						help = "See `bridgectl set-params --help` for more information!",
						"You didn't specify any bridge to set the parameters of!",
					),
				);
				std::process::exit(SET_PARAMS_NO_BRIDGE_FILTERS);
			}
		}

		if let Some(ip) = filter_ip {
			return ip;
		}

		match find_mion(
			if let Some(mac) = filter_mac {
				MIONFindBy::MacAddress(mac)
			} else {
				MIONFindBy::Name(filter_name.clone().unwrap_or_default())
			},
			false,
			Some(find_by_args.0),
			Some(find_by_args.1),
		)
		.await
		{
			Ok(Some(identity)) => identity.ip_address(),
			Ok(None) => {
				if use_json {
					error!(
					  id = "bridgectl::set_parameters::set_failed_to_find_a_device",
					  filter.ip = ?filter_ip,
					  filter.mac = ?filter_mac,
					  filter.name = ?filter_name,
					  suggestions = valuable(&[
						  "Please ensure the CAT-DEV you're trying to find is powered on, and running.",
						  "Make sure you are on the same Local Network, Subnet, and VLAN as the CAT-DEV device.",
						  "If you're not on the same VLAN, Subnet you can use something like: <https://github.com/udp-redux/udp-broadcast-relay-redux> to forward between the subnets & vlans.",
						  "Ensure your filters line up with a single CAT-DEV device.",
					  ]),
					);
				} else {
					error!(
						"\n{:?}",
						add_context_to(
							miette!(
								"Failed to find bridge that matched the series of filters, cannot set parameters.",
							),
							[
								miette!("Please ensure the CAT-DEV you're trying to find is powered on, and running."),
								miette!("Make sure you are on the same Local Network, Subnet, and VLAN as the CAT-DEV device."),
								miette!("If you're not on the same VLAN, Subnet you can use something like: <https://github.com/udp-redux/udp-broadcast-relay-redux> to forward between the subnets & vlans."),
								miette!(
									help = format!("Current Filter State: Bridge Filter IP: {filter_ip:?} / Bridge Filter Mac: {filter_mac:?} / Bridge Filter Name: {filter_name:?}"),
									"Ensure your filters line up with a single CAT-DEV device.",
								),
							].into_iter(),
						),
					);
				}
				std::process::exit(SET_PARAMS_NO_AVAILABLE_BRIDGE);
			}
			Err(cause) => {
				if use_json {
					error!(
						id = "bridgectl::set_parameters::failed_to_execute_broadcast",
						?cause,
						help = "Could not setup sockets to broadcast and search for the MION you specified; perhaps another program is already using the single MION port?",
					);
				} else {
					error!(
						"\n{:?}",
						miette!(
							help = "Perhaps another program is already using the single MION port?",
							"Could not setup sockets to broadcast and search for the MION you specified.",
						)
						.wrap_err(cause),
					);
				}
				std::process::exit(SET_PARAMS_NO_AVAILABLE_BRIDGE);
			}
		}
	} else {
		get_default_bridge_ip(use_json, host_state_path, find_by_args).await
	}
}

async fn get_mochiato_bridge_ip(
	use_json: bool,
	bridge_name: &str,
	find_by_args: (Duration, u16),
) -> Ipv4Addr {
	match find_mion(
		MIONFindBy::Name(bridge_name.to_owned()),
		false,
		Some(find_by_args.0),
		Some(find_by_args.1),
	)
	.await
	{
		Ok(Some(identity)) => identity.ip_address(),
		Ok(None) => {
			if use_json {
				error!(
					  id = "bridgectl::set_parameters::failed_to_find_ip_of_mochiato_bridge",
					  bridge.name = bridge_name,
					  suggestions = valuable(&[
						  "Please ensure the default CAT-DEV you're trying to find is powered on, and running.",
						  "Make sure you are on the same Local Network, Subnet, and VLAN as the CAT-DEV device.",
						  "If you're not on the same VLAN, Subnet you can use something like: <https://github.com/udp-redux/udp-broadcast-relay-redux> to forward between the subnets & vlans.",
						  "Ensure `cafe`/`cafex`/`mochiato` has been loaded with the latest information.",
					  ]),
					);
			} else {
				error!(
						"\n{:?}",
						add_context_to(
							miette!(
								"Failed to find the `cafe`/`cafex`/`mochiato` bridge's ip by broadcasting, and was not specified, cannot set parameters.",
							),
							[
								miette!("Please ensure the default CAT-DEV you're trying to find is powered on, and running."),
								miette!("Make sure you are on the same Local Network, Subnet, and VLAN as the CAT-DEV device."),
								miette!("If you're not on the same VLAN, Subnet you can use something like: <https://github.com/udp-redux/udp-broadcast-relay-redux> to forward between the subnets & vlans."),
								miette!("Ensure `cafe`/`cafex`/`mochiato` has been loaded with the latest information."),
							].into_iter(),
						),
					);
			}
			std::process::exit(SET_PARAMS_NO_AVAILABLE_BRIDGE);
		}
		Err(cause) => {
			if use_json {
				error!(
						id = "bridgectl::set_parameters::failed_to_execute_broadcast",
						?cause,
						help = "Could not setup sockets to broadcast and search for the default MION; perhaps another program is already using the single MION port?",
					);
			} else {
				error!(
					"\n{:?}",
					miette!(
						help = "Perhaps another program is already using the single MION port?",
						"Could not setup sockets to broadcast and search for the default MION.",
					)
					.wrap_err(cause),
				);
			}
			std::process::exit(SET_PARAMS_NO_AVAILABLE_BRIDGE);
		}
	}
}

async fn get_default_bridge_ip(
	use_json: bool,
	host_state_path: Option<PathBuf>,
	find_by_args: (Duration, u16),
) -> Ipv4Addr {
	let (default_bridge_name, opt_ip) = get_default_bridge(use_json, host_state_path.clone()).await;
	if let Some(ip) = opt_ip {
		ip
	} else {
		match find_mion(
			MIONFindBy::Name(default_bridge_name.clone()),
			false,
			Some(find_by_args.0),
			Some(find_by_args.1),
		)
		.await
		{
			Ok(Some(identity)) => identity.ip_address(),
			Ok(None) => {
				if use_json {
					error!(
					  id = "bridgectl::set_parameters::failed_to_find_ip_of_default_bridge",
					  bridge.name = default_bridge_name,
					  suggestions = valuable(&[
						  "Please ensure the default CAT-DEV you're trying to find is powered on, and running.",
						  "Make sure you are on the same Local Network, Subnet, and VLAN as the CAT-DEV device.",
						  "If you're not on the same VLAN, Subnet you can use something like: <https://github.com/udp-redux/udp-broadcast-relay-redux> to forward between the subnets & vlans.",
						  "Ensure your filters line up with a single CAT-DEV device.",
					  ]),
					);
				} else {
					error!(
						"\n{:?}",
						add_context_to(
							miette!(
								"Failed to find the default bridge's ip since the configuration file did not have it, cannot set parameters.",
							),
							[
								miette!("Please ensure the default CAT-DEV you're trying to find is powered on, and running."),
								miette!("Make sure you are on the same Local Network, Subnet, and VLAN as the CAT-DEV device."),
								miette!("If you're not on the same VLAN, Subnet you can use something like: <https://github.com/udp-redux/udp-broadcast-relay-redux> to forward between the subnets & vlans."),
								miette!(
									help = format!("Bridge Filter Path: {host_state_path:?}"),
									"Ensure your filters line up with a single CAT-DEV device.",
								),
							].into_iter(),
						),
					);
				}
				std::process::exit(SET_PARAMS_NO_AVAILABLE_BRIDGE);
			}
			Err(cause) => {
				if use_json {
					error!(
						id = "bridgectl::set_parameters::failed_to_execute_broadcast",
						?cause,
						help = "Could not setup sockets to broadcast and search for the default MION; perhaps another program is already using the single MION port?",
					);
				} else {
					error!(
						"\n{:?}",
						miette!(
							help = "Perhaps another program is already using the single MION port?",
							"Could not setup sockets to broadcast and search for the default MION.",
						)
						.wrap_err(cause),
					);
				}
				std::process::exit(SET_PARAMS_NO_AVAILABLE_BRIDGE);
			}
		}
	}
}
