//! Common helpers for dealing with bridge arguments.

use crate::{
	exit_codes::{
		ARGV_COULD_NOT_GET_DEFAULT_BRIDGE, GET_DEFAULT_CONFLICTING_FILTERS,
		GET_DEFAULT_WITH_FILTERS,
	},
	utils::{add_context_to, bridge_state_from_path, get_bridge_state_path},
};
use mac_address::MacAddress;
use miette::miette;
use std::{net::Ipv4Addr, path::PathBuf};
use tracing::{error, field::valuable};

/// Attempt to get the filters to use to "find" a bridge from all of the
/// arguments.
///
/// This will return `None` if you should just find the default bridge.
/// This will return `Some` if you need to execute some sort of broadcast
/// to find the bridge.
///
/// - `use_json`: if we're outputting json logs.
/// - `just_fetch_default`: if the user specified the equivalent of a
///   `--default` flag.
/// - `flag_arguments`: The three flag filtering arguments ip/mac/name.
/// - `positional_argument`: The positional argument that could be the bridge
///   name.
/// - `positional_specified_is_bridge_name`: Some commands may have argv1 be
///   a bridge name, but ALTERNATIVELY it could be param 2, and they just used
///   flags to filter the bridge. This boolean lets you denote whether they
///   actually specified it as a name.
///
/// ## Panics
///
/// This function will exit/panic the program if the user specified conflicting
/// arguments (e.g. flag + positional of the same field).
pub fn coalesce_bridge_arguments(
	use_json: bool,
	just_fetch_default: bool,
	flag_arguments: (Option<Ipv4Addr>, Option<String>, Option<String>),
	positional_argument: Option<String>,
	positional_specified_is_bridge_name: bool,
) -> Option<(Option<Ipv4Addr>, Option<MacAddress>, Option<String>)> {
	if just_fetch_default {
		if flag_arguments.0.is_some()
			|| flag_arguments.1.is_some()
			|| flag_arguments.2.is_some()
			|| positional_specified_is_bridge_name
		{
			if use_json {
				error!(
				  id = "bridgectl::argv::no_filters_allowed_on_default",
				  flags.ip = ?flag_arguments.0,
				  flags.mac = ?flag_arguments.1,
				  flags.name = ?flag_arguments.2,
				  args.positional = ?positional_argument,
				  suggestions = valuable(&[
					  "If you want to fetch the default bridge all you need to do is run: `bridgectl get-parameters <parameters> --default`",
					  "If you want to apply extra filtering you can do something like outputting JSON, and using `jq` to filter",
				  ]),
				);
			} else {
				error!(
          "\n{:?}",
					add_context_to(
						miette!("Cannot specify filters when fetching the default bridge!"),
						[
							miette!("If you want to fetch the default bridge all you need to do is run: `bridgectl get-parameters <parameters> --default`."),
							miette!(
								help = format!(
									"Flags: (--ip: `{:?}`, --mac: `{:?}`, --name: `{:?}`) / Positional Argument: `{:?}`",
									flag_arguments.0,
									flag_arguments.1,
									flag_arguments.2,
									positional_argument,
								),
								"If you want to apply extra filtering you can do something like outputting JSON, and using `jq` to filter.",
							),
						].into_iter(),
					),
        );
			}

			std::process::exit(GET_DEFAULT_WITH_FILTERS);
		}

		None
	} else {
		Some(coalesce_bridge_search_arguments(
			use_json,
			flag_arguments,
			positional_argument,
		))
	}
}

/// Get the default bridge, or exit out if there is no default bridge
/// configured.
///
/// ## Panics
///
/// Will panic if there is no default bridge that has been configured.
pub async fn get_default_bridge(
	use_json: bool,
	host_state_path: Option<PathBuf>,
) -> (String, Option<Ipv4Addr>) {
	let bridge_state =
		bridge_state_from_path(get_bridge_state_path(&host_state_path, use_json), use_json).await;

	if let Some(default_bridge) = bridge_state.get_default_bridge() {
		default_bridge
	} else if use_json {
		error!(
		  id = "bridgectl::argv::no_default_bridge",
		  host_state_path = %bridge_state.get_path().display(),
		  suggestions = valuable(&[
				"Please double check the configuration file located at the path specified, and ensure `BRIDGE_DEFAULT_NAME` is set to a real bridge name.",
				"If the bridge isn't set as the default you can use `bridge add --default <name> <ip>`, or `bridge set-default <'name' or 'ip'>`.",
		  ]),
		  "No default bridge present in configuration file.",
		);
		std::process::exit(ARGV_COULD_NOT_GET_DEFAULT_BRIDGE);
	} else {
		error!(
      "\n{:?}",
			add_context_to(
				miette!("No default bridge present in the configuration file."),
				[
					miette!("Please double check the configuration file located at the path specified, and ensure `BRIDGE_DEFAULT_NAME` is set to a real bridge name."),
					miette!(
						help = format!("The bridge configuration file was located at: {}", bridge_state.get_path().display()),
						"If the bridge isn't set as the default you can use `bridge add --default <name> <ip>`, or `bridge set-default <'name' or 'ip'>`.",
					),
				].into_iter(),
			),
    );
		std::process::exit(ARGV_COULD_NOT_GET_DEFAULT_BRIDGE);
	}
}

fn coalesce_bridge_search_arguments(
	use_json: bool,
	flag_arguments: (Option<Ipv4Addr>, Option<String>, Option<String>),
	cli_arguments: Option<String>,
) -> (Option<Ipv4Addr>, Option<MacAddress>, Option<String>) {
	let mac_flag = flag_arguments
		.1
		.and_then(|mac| MacAddress::try_from(mac.as_str()).ok());
	let Some(cli_arg) = cli_arguments else {
		return (flag_arguments.0, mac_flag, flag_arguments.2);
	};

	if let Ok(arg_as_ip) = cli_arg.parse::<Ipv4Addr>() {
		if flag_arguments.0.is_none() {
			return (Some(arg_as_ip), mac_flag, flag_arguments.2);
		}
	}
	if let Ok(arg_as_mac) = cli_arg.parse::<MacAddress>() {
		if mac_flag.is_none() {
			return (flag_arguments.0, Some(arg_as_mac), flag_arguments.2);
		}
	}
	if flag_arguments.2.is_none() {
		return (flag_arguments.0, mac_flag, Some(cli_arg));
	}

	if use_json {
		error!(
		  id = "bridgectl::argv::conflicting_filters_on_get",
		  flags.ip = ?flag_arguments.0,
		  flags.mac = ?mac_flag,
		  flags.name = ?flag_arguments.2,
		  args.positional = cli_arg,
		  suggestions = valuable(&[
			  "If the positional argument is a name, it may be trying to be parsed as something like an IP/Mac.",
				"Get bridge can only filter down to one bridge, if you're trying to apply multiple ip filters/name filters/mac filters either use multiple `bridgectl get` calls, or use something like `bridgectl list`.",
		  ]),
		);
	} else {
		error!(
      "\n{:?}",
			add_context_to(
				miette!("Positional argument conflicts with flag arguments!"),
				[
					miette!("If the positional argument is a name, it may be trying to be parsed as something like an IP/Mac."),
					miette!(
						help = format!(
							"Flags: (--ip: `{:?}`, --mac: `{:?}`, --name: `{:?}`) / Positional Argument: `{:?}`",
							flag_arguments.0,
							mac_flag,
							flag_arguments.2,
							cli_arg,
						),
						"Get bridge can only filter down to one bridge, if you're trying to apply multiple ip filters/name filters/mac filters either use multiple `bridgectl get` calls, or use something like `bridgectl list`.",
					),
				].into_iter(),
			),
    );
	}

	std::process::exit(GET_DEFAULT_CONFLICTING_FILTERS);
}
