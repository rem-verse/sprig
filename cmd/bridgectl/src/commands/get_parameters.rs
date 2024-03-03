use crate::{
	commands::argv_helpers::{coalesce_bridge_arguments, get_default_bridge},
	exit_codes::{
		GET_PARAMS_FAILED_TO_GET_PARAMS, GET_PARAMS_NO_AVAILABLE_BRIDGE,
		GET_PARAMS_NO_BRIDGE_FILTERS, GET_PARAMS_NO_PARAMETERS_SPECIFIED,
	},
	knobs::env::{BRIDGE_CURRENT_IP_ADDRESS, BRIDGE_CURRENT_NAME},
	utils::add_context_to,
};
use cat_dev::mion::{
	discovery::{find_mion, MIONFindBy},
	parameter::get_parameters,
	proto::parameter::DumpedMionParameters,
};
use miette::miette;
use std::{net::Ipv4Addr, path::PathBuf, time::Duration};
use tracing::{debug, error, field::valuable, info};

/// Actual command handler for the `get-parameters`, or `gp` command.
#[allow(
	// This is unfortunate that there are a lot, but the command accepts lots of
	// potential parameters.
	//
	// The parameters are also fairly different types, so the chances of screwing
	// up passing them in without noticing is low.
	clippy::too_many_arguments,
)]
pub async fn handle_get_parameters(
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
	};

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

	print_parameters(
		use_json,
		&param_filters,
		&fetch_parameters(use_json, bridge_ip, parameter_space_port).await,
	);
}

fn print_parameters(use_json: bool, parameter_filters: &str, parameters: &DumpedMionParameters) {
	for filter in parameter_filters.split(',') {
		if filter.is_empty() {
			if use_json {
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
			if use_json {
				info!(
					id = "bridgectl::get_parameters::parameter_found",
					line="Found parameter!",
					parameter.name=%filter,
					parameter.value=%parameter_value,
				);
			} else {
				info!(parameter.name=%filter, parameter.value=%parameter_value, "Found your parameter!");
			}
		} else if use_json {
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

async fn fetch_parameters(
	use_json: bool,
	bridge_ip: Ipv4Addr,
	bridge_port: Option<u16>,
) -> DumpedMionParameters {
	match get_parameters(bridge_ip, bridge_port, None).await {
		Ok(params) => params,
		Err(cause) => {
			if use_json {
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
					id = "bridgectl::get_parameters::no_bridge_filters",
					help = "You didn't specify any bridge to get the parameters of!",
				);
				std::process::exit(GET_PARAMS_NO_BRIDGE_FILTERS);
			} else {
				error!(
					"\n{:?}",
					miette!(
						help = "See `bridgectl get-params --help` for more information!",
						"You didn't specify any bridge to get the parameters of!",
					),
				);
				std::process::exit(GET_PARAMS_NO_BRIDGE_FILTERS);
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
					  id = "bridgectl::get_parameters::get_failed_to_find_a_device",
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
								"Failed to find bridge that matched the series of filters, cannot get parameters.",
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
				std::process::exit(GET_PARAMS_NO_AVAILABLE_BRIDGE);
			}
			Err(cause) => {
				if use_json {
					error!(
						id = "bridgectl::get_parameters::failed_to_execute_broadcast",
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
				std::process::exit(GET_PARAMS_NO_AVAILABLE_BRIDGE);
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
					  id = "bridgectl::get_parameters::failed_to_find_ip_of_mochiato_bridge",
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
								"Failed to find the `cafe`/`cafex`/`mochiato` bridge's ip by broadcasting, and was not specified, cannot get parameters.",
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
			std::process::exit(GET_PARAMS_NO_AVAILABLE_BRIDGE);
		}
		Err(cause) => {
			if use_json {
				error!(
						id = "bridgectl::get_parameters::failed_to_execute_broadcast",
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
			std::process::exit(GET_PARAMS_NO_AVAILABLE_BRIDGE);
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
					  id = "bridgectl::get_parameters::failed_to_find_ip_of_default_bridge",
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
								"Failed to find the default bridge's ip since the configuration file did not have it, cannot get parameters.",
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
				std::process::exit(GET_PARAMS_NO_AVAILABLE_BRIDGE);
			}
			Err(cause) => {
				if use_json {
					error!(
						id = "bridgectl::get_parameters::failed_to_execute_broadcast",
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
				std::process::exit(GET_PARAMS_NO_AVAILABLE_BRIDGE);
			}
		}
	}
}
