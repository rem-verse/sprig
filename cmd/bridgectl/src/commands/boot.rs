//! Perform booting of a cat-dev bridge.
//!
//! TODO(mythra): not fully implemented yet.

use crate::{
	commands::argv_helpers::{coalesce_bridge_arguments, get_default_bridge},
	exit_codes::{
		BOOT_CGI_FAILURE, BOOT_NO_AVAILABLE_BRIDGE, BOOT_NO_BRIDGE_FILTERS, NOT_YET_IMPLEMENTED,
	},
	knobs::env::{BRIDGE_CURRENT_IP_ADDRESS, BRIDGE_CURRENT_NAME},
	utils::add_context_to,
};
use cat_dev::mion::{
	cgis::very_hacky_will_break_dont_use_power_on,
	discovery::{find_mion, MIONFindBy},
};
use mac_address::MacAddress;
use miette::miette;
use std::{net::Ipv4Addr, path::PathBuf, time::Duration};
use tracing::{error, field::valuable, info, warn};

pub async fn handle_boot(
	use_json: bool,
	just_fetch_default: bool,
	bridge_flag_arguments: (Option<Ipv4Addr>, Option<String>, Option<String>),
	bridge_argv: Option<String>,
	find_by_args: (Duration, u16),
	host_state_path: Option<PathBuf>,
	no_pcfs: bool,
) {
	let bridge_ip = get_bridge_ip(
		use_json,
		just_fetch_default,
		bridge_flag_arguments,
		bridge_argv,
		find_by_args,
		host_state_path,
	)
	.await;

	if no_pcfs {
		warn!("NOTE: this will work if the cat-dev hasn't been taken control of by another host, but does not have good error handling yet!");
		match very_hacky_will_break_dont_use_power_on(bridge_ip).await {
			Ok(result_code) => {
				if result_code {
					info!("Successfully powered on MION!");
				} else {
					error!("Failed to boot cat-dev bridge! Please reach out for support!");
					std::process::exit(BOOT_CGI_FAILURE);
				}
			}
			Err(cause) => {
				if use_json {
					error!(
						id = "bridgectl::boot::failed_to_boot_device",
						bridge.ip = %bridge_ip,
						?cause,
						suggestions = valuable(&[
						  "Please file an issue, and reach out!"
						]),
					);
				} else {
					error!(
				  	"\n{:?}",
				  	miette!(
				  		help = "PLEASE PLEASE PLEASE FILE AN ISSUE!",
				  		"Failure to perform hacky non-emulated boot!!! THIS IS STILL EARLY !!! PLEASE FILE AN ISSUE!\n {cause:?}",
				  	),
				  );
				}

				std::process::exit(BOOT_CGI_FAILURE);
			}
		}
	} else {
		error!("Sorry! THIS HAS NOT YET BEEN IMPLEMENTED! FSEMUL IS MAKING ME CRY!");
		std::process::exit(NOT_YET_IMPLEMENTED);
	}
}

async fn get_bridge_ip(
	use_json: bool,
	just_fetch_default: bool,
	bridge_flag_arguments: (Option<Ipv4Addr>, Option<String>, Option<String>),
	bridge_argv: Option<String>,
	find_by_args: (Duration, u16),
	host_state_path: Option<PathBuf>,
) -> Ipv4Addr {
	let did_specify_cli_arg = bridge_argv.is_some();
	if let Some((filter_ip, filter_mac, filter_name)) = coalesce_bridge_arguments(
		use_json,
		just_fetch_default,
		bridge_flag_arguments,
		bridge_argv,
		did_specify_cli_arg,
	) {
		if filter_ip.is_none() && filter_mac.is_none() && filter_name.is_none() {
			if let Some(ip) = BRIDGE_CURRENT_IP_ADDRESS.as_ref() {
				*ip
			} else if let Some(bn) = BRIDGE_CURRENT_NAME.as_ref() {
				get_mochiato_bridge_ip(use_json, bn, find_by_args).await
			} else if use_json {
				error!(
					id = "bridgectl::boot::no_bridge_filters",
					help = "You didn't specify any bridge to get the information of!",
				);
				std::process::exit(BOOT_NO_BRIDGE_FILTERS);
			} else {
				error!(
					"\n{:?}",
					miette!(
						help = "See `bridgectl boot --help` for more information!",
						"You didn't specify any bridge to get the information of!",
					),
				);
				std::process::exit(BOOT_NO_BRIDGE_FILTERS);
			}
		} else {
			find_bridge_ip_from_args(use_json, filter_ip, filter_mac, filter_name, find_by_args)
				.await
		}
	} else {
		get_default_bridge_ip(use_json, host_state_path, find_by_args).await
	}
}

async fn find_bridge_ip_from_args(
	use_json: bool,
	filter_ip: Option<Ipv4Addr>,
	filter_mac: Option<MacAddress>,
	filter_name: Option<String>,
	find_by_args: (Duration, u16),
) -> Ipv4Addr {
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
				  id = "bridgectl::boot::set_failed_to_find_a_device",
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
  						"Failed to find bridge that matched the series of filters, cannot boot.",
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
			std::process::exit(BOOT_NO_AVAILABLE_BRIDGE);
		}
		Err(cause) => {
			if use_json {
				error!(
  				id = "bridgectl::boot::failed_to_execute_broadcast",
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
			std::process::exit(BOOT_NO_AVAILABLE_BRIDGE);
		}
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
					  id = "bridgectl::boot::failed_to_find_ip_of_mochiato_bridge",
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
								"Failed to find the `cafe`/`cafex`/`mochiato` bridge's ip by broadcasting, and was not specified, cannot boot device.",
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
			std::process::exit(BOOT_NO_AVAILABLE_BRIDGE);
		}
		Err(cause) => {
			if use_json {
				error!(
						id = "bridgectl::boot::failed_to_execute_broadcast",
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
			std::process::exit(BOOT_NO_AVAILABLE_BRIDGE);
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
					  id = "bridgectl::boot::failed_to_find_ip_of_default_bridge",
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
								"Failed to find the default bridge's ip since the configuration file did not have it, cannot boot.",
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
				std::process::exit(BOOT_NO_AVAILABLE_BRIDGE);
			}
			Err(cause) => {
				if use_json {
					error!(
						id = "bridgectl::boot::failed_to_execute_broadcast",
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
				std::process::exit(BOOT_NO_AVAILABLE_BRIDGE);
			}
		}
	}
}
