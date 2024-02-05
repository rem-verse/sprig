use crate::{
	commands::argv_helpers::{coalesce_bridge_arguments, get_default_bridge},
	exit_codes::{DUMP_PARAMS_FAILED_TO_GET_PARAMS, DUMP_PARAMS_NO_AVAILABLE_BRIDGE},
	utils::add_context_to,
};
use cat_dev::mion::{
	discovery::{find_mion, MIONFindBy},
	parameter::get_parameters,
	proto::parameter::DumpedMionParameters,
};
use miette::miette;
use std::{net::Ipv4Addr, path::PathBuf};
use tracing::{error, field::valuable, info};

/// Actual command handler for the `dump-parameters`, or `dp` command.
pub async fn handle_dump_parameters(
	use_json: bool,
	just_fetch_default: bool,
	bridge_flag_arguments: (Option<Ipv4Addr>, Option<String>, Option<String>),
	bridge_argv: Option<String>,
	host_state_path: Option<PathBuf>,
) {
	let had_arg = bridge_argv.is_some();
	let bridge_ip = get_a_bridge_ip(
		use_json,
		just_fetch_default,
		bridge_flag_arguments,
		bridge_argv,
		had_arg,
		host_state_path,
	)
	.await;
	print_parameters(use_json, &fetch_parameters(use_json, bridge_ip).await);
}

fn print_parameters(use_json: bool, parameters: &DumpedMionParameters) {
	if !use_json {
		info!("\n\nDumping Parameter Space:");
	}

	for (chunk_idx, chunk) in parameters.get_raw_parameters().chunks(16).enumerate() {
		if use_json {
			let mut ascii_str = String::with_capacity(16);
			let mut bytes = Vec::with_capacity(16);

			for byte in chunk {
				bytes.push(byte);
				let as_char = *byte as char;
				if as_char.is_ascii_alphanumeric() {
					ascii_str.push(as_char);
				} else {
					ascii_str.push('.');
				}
			}

			info!(
			  id = "bridgectl::dump_parameters::dump_line",
			  %ascii_str,
			  ?bytes,
			);
		} else {
			print!("  {chunk_idx:02x}0: ");
			let mut ascii_str = String::with_capacity(16);
			for byte in chunk {
				print!("{byte:02x} ");
				let as_char = *byte as char;
				if as_char.is_ascii_alphanumeric() {
					ascii_str.push(as_char);
				} else {
					ascii_str.push('.');
				}
			}
			println!("    {ascii_str}");
		}
	}
}

async fn fetch_parameters(use_json: bool, bridge_ip: Ipv4Addr) -> DumpedMionParameters {
	match get_parameters(bridge_ip, None).await {
		Ok(params) => params,
		Err(cause) => {
			if use_json {
				error!(
					id = "bridgectl::dump_parameters::failed_to_execute_dump_parameters",
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
			std::process::exit(DUMP_PARAMS_FAILED_TO_GET_PARAMS);
		}
	}
}

async fn get_a_bridge_ip(
	use_json: bool,
	just_fetch_default: bool,
	bridge_flag_arguments: (Option<Ipv4Addr>, Option<String>, Option<String>),
	bridge_or_params_argument: Option<String>,
	had_params_arg: bool,
	host_state_path: Option<PathBuf>,
) -> Ipv4Addr {
	if let Some((filter_ip, filter_mac, filter_name)) = coalesce_bridge_arguments(
		use_json,
		just_fetch_default,
		bridge_flag_arguments,
		bridge_or_params_argument,
		had_params_arg,
	) {
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
			None,
		)
		.await
		{
			Ok(Some(identity)) => identity.ip_address(),
			Ok(None) => {
				if use_json {
					error!(
					  id = "bridgectl::dump_parameters::get_failed_to_find_a_device",
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
				std::process::exit(DUMP_PARAMS_NO_AVAILABLE_BRIDGE);
			}
			Err(cause) => {
				if use_json {
					error!(
						id = "bridgectl::dump_parameters::failed_to_execute_broadcast",
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
				std::process::exit(DUMP_PARAMS_NO_AVAILABLE_BRIDGE);
			}
		}
	} else {
		get_default_bridge_ip(use_json, host_state_path).await
	}
}

async fn get_default_bridge_ip(use_json: bool, host_state_path: Option<PathBuf>) -> Ipv4Addr {
	let (default_bridge_name, opt_ip) = get_default_bridge(use_json, host_state_path.clone()).await;
	if let Some(ip) = opt_ip {
		ip
	} else {
		match find_mion(MIONFindBy::Name(default_bridge_name.clone()), false, None).await {
			Ok(Some(identity)) => identity.ip_address(),
			Ok(None) => {
				if use_json {
					error!(
					  id = "bridgectl::dump_parameters::failed_to_find_ip_of_default_bridge",
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
				std::process::exit(DUMP_PARAMS_NO_AVAILABLE_BRIDGE);
			}
			Err(cause) => {
				if use_json {
					error!(
						id = "bridgectl::dump_parameters::failed_to_execute_broadcast",
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
				std::process::exit(DUMP_PARAMS_NO_AVAILABLE_BRIDGE);
			}
		}
	}
}
