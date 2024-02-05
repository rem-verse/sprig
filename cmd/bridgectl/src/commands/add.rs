//! Handles the `add`, or `update` command for `bridgectl`.

use crate::{
	exit_codes::{
		ADD_COULD_NOT_FIND, ADD_COULD_NOT_SAVE_TO_DISK, ADD_COULD_NOT_SEARCH, ADD_COULD_NOT_UPSERT,
		CONFLICTING_ARGUMENTS_FOR_ADD, NO_SPECIFIER_FOR_ADD,
	},
	utils::{add_context_to, bridge_state_from_path},
};
use cat_dev::{
	mion::discovery::{discover_bridges, find_mion, MIONFindBy},
	BridgeHostState,
};
use miette::miette;
use std::{net::Ipv4Addr, path::PathBuf};
use tracing::{error, field::valuable, info};

/// Handle adding a bridge, or updating a bridge.
pub async fn handle_add_or_update(
	use_json: bool,
	cli_arguments: (Option<String>, Option<Ipv4Addr>),
	positional_arguments: (Option<String>, Option<Ipv4Addr>),
	host_state_path: PathBuf,
	set_default: bool,
) {
	let (name_arg, ip_arg) = get_argv_from_all(use_json, cli_arguments, positional_arguments);

	let (bridge_name, bridge_ip): (String, Ipv4Addr) = if name_arg.is_none() && ip_arg.is_none() {
		if use_json {
			error!(
				id = "bridgectl::add::missing_all_info",
				cause = "No name argument, or ip argument were specified one of which is needed for add.",
				suggestions = valuable(&[
					"You can specify the argument with positional arguments like: `bridgectl add <name> <ip>`",
					"You can specify the argument with flags like: `bridgectl add --name <name> --bridge-ip <ip>`",
					"note: you only need to specify one of these arguments.",
				]),
			);
		} else {
			error!(
				"\n{:?}",
				add_context_to(
					miette!("No name, or ip argument were specified! We need at least one in order to search!"),
					[
						miette!(
							help = "You can run `bridgectl add --help` for more information.",
							"`bridgectl add --name <name> --bridge-ip <ip>`, or `bridgectl add <name> <ip>` you only need to specify one.",
						),
					].into_iter(),
				),
			);
		}

		std::process::exit(NO_SPECIFIER_FOR_ADD);
	} else if let Some(bn_arg) = name_arg {
		if let Some(bi_arg) = ip_arg {
			(bn_arg, bi_arg)
		} else {
			// This could be a name or an IP techincally. we try matching on both.
			mion_find_by_name_or_ip(use_json, bn_arg).await
		}
	} else if let Some(bi_arg) = ip_arg {
		(mion_find_name_from_ip(use_json, bi_arg).await, bi_arg)
	} else {
		// double non check above.
		unreachable!()
	};

	let bridge_host_state = bridge_state_from_path(host_state_path, use_json).await;
	upsert_bridge(
		use_json,
		set_default,
		bridge_name,
		bridge_ip,
		bridge_host_state,
	)
	.await;
}

async fn upsert_bridge(
	use_json: bool,
	set_default: bool,
	bridge_name: String,
	bridge_ip: Ipv4Addr,
	mut host_state: BridgeHostState,
) {
	if let Err(cause) = host_state.upsert_bridge(&bridge_name, bridge_ip) {
		if use_json {
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
		if use_json {
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
		if set_default {
			" And successfully set it as your default bridge."
		} else { "" }
	);
}

/// Given a name (or potentially an IP) find the appropriate information.
async fn mion_find_by_name_or_ip(use_json: bool, bridge_name_or_ip: String) -> (String, Ipv4Addr) {
	let potential_ip_arg = bridge_name_or_ip.parse::<Ipv4Addr>().ok();
	let mut recv_channel = match discover_bridges(false).await {
		Ok(channel) => channel,
		Err(cause) => {
			if use_json {
				error!(
					id = "bridgectl::add::failed_to_execute_broadcast",
					?cause,
					help = "Could not setup sockets to broadcast and search for all MIONs; perhaps another program is already using the single MION port?",
				);
			} else {
				error!(
					"\n{:?}",
					miette!(
						"Could not setup sockets to broadcast and search for all MIONs; perhaps another program is already using the single MION port?",
					).wrap_err(cause),
				);
			}

			std::process::exit(ADD_COULD_NOT_SEARCH);
		}
	};

	while let Some(identity) = recv_channel.recv().await {
		if identity.name() == bridge_name_or_ip.as_str()
			|| Some(identity.ip_address()) == potential_ip_arg
		{
			return (identity.name().to_owned(), identity.ip_address());
		}
	}

	// Didn't find one that matches your args.
	if use_json {
		error!(
			id = "bridgectl::add::failed_to_find_a_device",
			search_for.name = bridge_name_or_ip,
			search_for.ip = ?potential_ip_arg,
			suggestions = valuable(&[
				"Please ensure the CAT-DEV is powered on, and running.",
				"Make sure you are on the same Local Network, Subnet, and VLAN as the CAT-DEV device.",
				"If you're not on the same VLAN, Subnet you can use something like: <https://github.com/udp-redux/udp-broadcast-relay-redux> to forward between the subnets & vlans.",
			]),
			"Could not find a bridge searching by the name, or by ip."
		);
	} else {
		error!(
			"\n{:?}",
			add_context_to(
				miette!("Could not find a bridge by the name, or potentially an ip."),
				[
					miette!("Please ensure the CAT-DEV is powered on, and running."),
					miette!("Make sure you are on the same Local Network, Subnet, and VLAN as the CAT-DEV device."),
					miette!(
						help = format!("Searching for Name: {bridge_name_or_ip} / searched for ip: {potential_ip_arg:?}"),
						"If you're not on the same VLAN, Subnet you can use something like: <https://github.com/udp-redux/udp-broadcast-relay-redux> to forward between the two VLANs/Subnets.",
					),
				].into_iter(),
			),
		);
	}

	std::process::exit(ADD_COULD_NOT_FIND);
}

/// Given a guaranteed IP Argument, find the bridges name.
async fn mion_find_name_from_ip(use_json: bool, bridge_ip: Ipv4Addr) -> String {
	match find_mion(MIONFindBy::Ip(bridge_ip), false, None).await {
		Ok(Some(bridge)) => bridge.name().to_owned(),
		Ok(None) => {
			if use_json {
				error!(
					id = "bridgectl::add::failed_to_find_a_device",
					ip = %bridge_ip,
					help = "Could not send packet directly to MION perhaps it's not on, or reachable?",
				);
			} else {
				error!(
					"\n{:?}",
					miette!(
						help=format!("Try loading up the page: <http://{bridge_ip}/menu.cgi> to see if the bridge is online and you can talk with and connect too the bridge."),
						"Could not send a packet directly to the IP specified, perhaps it wasn't reachable?",
					),
				);
			}

			std::process::exit(ADD_COULD_NOT_FIND);
		}
		Err(cause) => {
			if use_json {
				error!(
					id = "bridgectl::add::failed_to_execute_search",
					?cause,
					ip = %bridge_ip,
					help = "Could not send packet directly to MION perhaps it's not on, or reachable?",
				);
			} else {
				error!(
					"\n{:?}",
					miette!(
						help=format!("Try loading up the page: <http://{bridge_ip}/menu.cgi> to see if the bridge is online and you can talk with and connect too the bridge."),
						"Could not send a packet directly to the IP specified, perhaps it wasn't reachable?",
					).wrap_err(cause),
				);
			}
			std::process::exit(ADD_COULD_NOT_SEARCH);
		}
	}
}

/// Get the proper combination of arguments between Positional Arguments, and
/// Flagged Arguments.
fn get_argv_from_all(
	use_json: bool,
	cli_arguments: (Option<String>, Option<Ipv4Addr>),
	positional_arguments: (Option<String>, Option<Ipv4Addr>),
) -> (Option<String>, Option<Ipv4Addr>) {
	if cli_arguments.1.is_some() && positional_arguments.1.is_some() {
		if use_json {
			error!(
				id = "bridgectl::cli::conflicting_arguments",
				flagged_argument = %cli_arguments.1.unwrap_or(Ipv4Addr::LOCALHOST),
				positional_argument = %positional_arguments.1.unwrap_or(Ipv4Addr::LOCALHOST),
				"IP was specified with a command line argument, and a flag argument creating conflicting bits of info. Please only specify it once."
			);
		} else {
			error!(
				"\n{:?}",
				add_context_to(
					miette!("Bridge IP was specified twice (with a cli flag, and a positional argument) creating conflicting bits of information."),
					[
						miette!("You can run `bridgectl add --help` to see how to specify arguments."),
						miette!("You only need to specify the name or the ip address, and you only need to specify it once."),
						miette!(
							help=format!("Bridge IP Flag Value: {:?} / Bridge IP Positional Argument Value: {:?}", cli_arguments.1, positional_arguments.1),
							"Even if these values are the same value, please only specify it once.",
						),
					].into_iter(),
				),
			);
		}
		std::process::exit(CONFLICTING_ARGUMENTS_FOR_ADD);
	}

	let mut ip_argument = cli_arguments.1;
	if ip_argument.is_none() {
		ip_argument = positional_arguments.1;
	}

	if cli_arguments.0.is_some() && cli_arguments.1.is_none() {
		if let Some(ip_value) = positional_arguments
			.0
			.as_ref()
			.and_then(|val| val.parse::<Ipv4Addr>().ok())
		{
			ip_argument = Some(ip_value);
		} else {
			if use_json {
				error!(
					id = "bridgectl::cli::conflicting_arguments",
					flagged_argument = cli_arguments.0,
					positional_argument = positional_arguments.0,
					"Name was specified with a command line argument, and a flag argument creating conflicting bits of info. Please only specify it once."
				);
			} else {
				error!(
					"\n{:?}",
					add_context_to(
						miette!("Bridge Name was specified twice (with a cli flag, and a positional argument) creating conflicting bits of information."),
						[
							miette!("You can run `bridgectl add --help` to see how to specify arguments."),
							miette!("You only need to specify the name or the ip address, and you only need to specify it once."),
							miette!(
								help=format!("Bridge Name Flag Value: {:?} / Bridge Name Positional Argument Value: {:?}", cli_arguments.0, positional_arguments.0),
								"Even if these values are the same value, please only specify it once.",
							),
						].into_iter(),
					),
				);
			}
			std::process::exit(CONFLICTING_ARGUMENTS_FOR_ADD);
		}
	}
	let mut name_argument = cli_arguments.0;
	if name_argument.is_none() {
		name_argument = positional_arguments.0;
	}

	(name_argument, ip_argument)
}
