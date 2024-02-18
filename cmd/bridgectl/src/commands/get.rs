//! Handles fetching the information for just one particular bridge.

use crate::{
	commands::argv_helpers::{coalesce_bridge_arguments, get_default_bridge, get_padded_string},
	exit_codes::{
		GET_FAILED_TO_FIND_SPECIFIC_DEVICE, GET_FAILED_TO_SEARCH_FOR_DEVICE, GET_NO_BRIDGE_FILTERS,
	},
	knobs::env::{BRIDGE_CURRENT_IP_ADDRESS, BRIDGE_CURRENT_NAME},
	utils::{add_context_to, bridge_state_from_path, get_bridge_state_path},
};
use cat_dev::{
	errors::CatBridgeError,
	mion::{
		discovery::{discover_bridges, find_mion, MIONFindBy},
		proto::control::MionIdentity,
	},
};
use mac_address::MacAddress;
use miette::miette;
use std::{net::Ipv4Addr, path::PathBuf};
use terminal_size::{terminal_size, Width as TermWidth};
use tracing::{debug, error, field::valuable, info, warn};
use valuable::Value as ValuableValue;

const DEFAULT_HEADER: &str = "Bridge Name                    | IP Address      ";
const DEFAULT_HEADER_LINE: &str = "-------------------------------------------------";

const FALLBACK_HEADER: &str = "Bridge Name                    | IP Address      | Is Default";
const FALLBACK_HEADER_LINE: &str = "-------------------------------------------------------------";

const DETAILED_HEADER: &str =      "Bridge Name                    | IP Address      | MAC Address        | FPGA image version | Firmware Version | SDK Version | Boot Mode | Power Status";
const DETAILED_HEADER_LINE: &str = "------------------------------------------------------------------------------------------------------------------------------------------------------";

/// Actual command handler for the `get` command.
pub async fn handle_get(
	use_json: bool,
	use_table: bool,
	just_fetch_default: bool,
	flag_arguments: (Option<Ipv4Addr>, Option<String>, Option<String>),
	cli_arguments: Option<String>,
	host_state_path: Option<PathBuf>,
) {
	let did_specify_cli_arg = cli_arguments.is_some();
	if let Some((filter_ip, filter_mac, filter_name)) = coalesce_bridge_arguments(
		use_json,
		just_fetch_default,
		flag_arguments,
		cli_arguments,
		did_specify_cli_arg,
	) {
		if filter_ip.is_none() && filter_mac.is_none() && filter_name.is_none() {
			if BRIDGE_CURRENT_NAME.is_some() || BRIDGE_CURRENT_IP_ADDRESS.is_some() {
				print_mochiato_bridge(
					use_json,
					use_table,
					BRIDGE_CURRENT_NAME.clone(),
					*BRIDGE_CURRENT_IP_ADDRESS,
				)
				.await;
			} else if use_json {
				error!(
					id = "bridgectl::get::no_bridge_filters",
					help = "You didn't specify any bridge to get the information of!",
				);
				std::process::exit(GET_NO_BRIDGE_FILTERS);
			} else {
				error!(
					"\n{:?}",
					miette!(
						help = "See `bridgectl get --help` for more information!",
						"You didn't specify any bridge to get the information of!",
					),
				);
				std::process::exit(GET_NO_BRIDGE_FILTERS);
			}
		} else {
			print_bridge(
				use_json,
				use_table,
				filter_ip,
				filter_mac,
				filter_name,
				host_state_path,
			)
			.await;
		}
	} else {
		print_default_bridge(use_json, use_table, host_state_path).await;
	}
}

async fn print_mochiato_bridge(
	use_json: bool,
	use_table: bool,
	env_name: Option<String>,
	env_ip: Option<Ipv4Addr>,
) {
	if use_json {
		info!(
			id   = "bridgectl::get::mochiato_bridge_detailed_lookup",
			line = "Found `cafe`/`cafex`/`mochiato` environment variables for bridge, attempting to lookup detailed information to print.",
		);
	} else {
		info!(
			"Found `cafe`/`cafex`/`mochiato` environment variables for bridge, attempting to lookup detailed information..."
		);
	}

	match find_identity_from_network(env_ip, None, env_name.as_deref()).await {
		Ok(Some(identity)) => {
			print_detailed_bridge(use_json, use_table, &identity);
		}
		Ok(None) => {
			if use_json {
				warn!(
					id = "bridgectl::get::find_mochiato_bridge_failed",
					line = "find_identity_from_network returned Ok(None) for cafe/cafex/mochiato environment-variable bridge.",
				);
			} else {
				warn!("Could not find the bridge specified in the environment by `cafe`/`cafex`/`mochiato`, perhaps it is not running, or `cafe` has bugged? Printing out known static information.");
			}
		}
		Err(cause) => {
			if use_json {
				warn!(
					id = "bridgectl::get::failed_to_execute_broadcast",
					?cause,
					help = "Could not setup sockets to broadcast and search for detailed information; perhaps another program is already using the single MION port? Trying to find MION from config file (will be less detailed).",
				);
			} else {
				warn!(
				"\n{:?}",
				miette!(
					help = "Perhaps another program is already using the single MION port?",
					"Could not setup sockets to broadcast and search for detailed information on the default MION (trying to search config file, will be less detailed).",
				).wrap_err(cause),
			);
			}
		}
	}

	if use_table {
		let rendered_name = get_padded_string(env_name.as_deref().unwrap_or("<missing data>"), 30);
		let rendered_ip = get_padded_string(
			env_ip.map_or("<missing data>".to_owned(), |bip| format!("{bip}")),
			15,
		);
		let full_line = format!("{rendered_name} | {rendered_ip}");

		if use_json {
			info!(
				id = "bridgectl::get::mochiato_bridge_table",
				line = DEFAULT_HEADER
			);
			info!(
				id = "bridgectl::get::mochiato_bridge_table",
				line = DEFAULT_HEADER_LINE
			);
			info!(
			  id = "bridgectl::get::mochiato_bridge_table",
			  line = full_line,
			  bridge.name = ?env_name,
			  bridge.ip = ?env_ip,
			);
		} else {
			println!("{DEFAULT_HEADER}");
			println!("{DEFAULT_HEADER_LINE}");
			println!("{full_line}");
		}
	} else if use_json {
		info!(
			id = "bridgectl::get::mochiato_bridge",
			bridge.ip = env_ip.map_or(String::new(), |ip| format!("{ip}")),
			bridge.name = env_name,
		);
	} else {
		info!(
			bridge.ip = env_ip.map_or(String::new(), |ip| format!("{ip}")),
			bridge.name = env_name,
			"Found bridge specified by `cafe`/`cafex`/`mochiato`!",
		);
	}
}

async fn print_default_bridge(use_json: bool, use_table: bool, host_state_path: Option<PathBuf>) {
	let (default_bridge_name, opt_bridge_ip) = get_default_bridge(use_json, host_state_path).await;
	if use_json {
		info!(
			id = "bridgectl::get::default_bridge_detailed_lookup",
			line = "Found default bridge, attempting to lookup detailed information to print.",
		);
	} else {
		info!(
			"Found default bridge in configuration, attempting to lookup detailed information..."
		);
	}

	match find_identity_from_network(opt_bridge_ip, None, Some(default_bridge_name.as_str())).await
	{
		Ok(Some(identity)) => {
			print_detailed_bridge(use_json, use_table, &identity);
		}
		Ok(None) => {
			if use_json {
				warn!(
					id = "bridgectl::get::find_default_identity_failed",
					line = "find_identity_from_network returned Ok(None) for default bridge.",
				);
			} else {
				warn!("Could not find default bridge, perhaps it is not running, or the file contents are incorrect? Printing out known static information.");
			}
		}
		Err(cause) => {
			if use_json {
				warn!(
					id = "bridgectl::get::failed_to_execute_broadcast",
					?cause,
					help = "Could not setup sockets to broadcast and search for detailed information; perhaps another program is already using the single MION port? Trying to find MION from config file (will be less detailed).",
				);
			} else {
				warn!(
				"\n{:?}",
				miette!(
					help = "Perhaps another program is already using the single MION port?",
					"Could not setup sockets to broadcast and search for detailed information on the default MION (trying to search config file, will be less detailed).",
				).wrap_err(cause),
			);
			}
		}
	}

	if use_table {
		let rendered_name = get_padded_string(&default_bridge_name, 30);
		let rendered_ip = get_padded_string(
			opt_bridge_ip.map_or("<missing data>".to_owned(), |bip| format!("{bip}")),
			15,
		);
		let full_line = format!("{rendered_name} | {rendered_ip}");

		if use_json {
			info!(
				id = "bridgectl::get::default_bridge_table",
				line = DEFAULT_HEADER
			);
			info!(
				id = "bridgectl::get::default_bridge_table",
				line = DEFAULT_HEADER_LINE
			);
			info!(
			  id = "bridgectl::get::default_bridge_table",
			  line = full_line,
			  bridge.name = default_bridge_name,
			  bridge.ip = ?opt_bridge_ip,
			);
		} else {
			println!("{DEFAULT_HEADER}");
			println!("{DEFAULT_HEADER_LINE}");
			println!("{full_line}");
		}
	} else if use_json {
		info!(
			id = "bridgectl::get::default_bridge",
			bridge.ip = opt_bridge_ip.map_or(String::new(), |ip| format!("{ip}")),
			bridge.name = default_bridge_name,
		);
	} else {
		info!(
			bridge.ip = opt_bridge_ip.map_or(String::new(), |ip| format!("{ip}")),
			bridge.name = default_bridge_name,
			"Found default bridge!",
		);
	}
}

async fn print_bridge(
	use_json: bool,
	use_table: bool,
	filter_ip: Option<Ipv4Addr>,
	filter_mac: Option<MacAddress>,
	filter_name: Option<String>,
	bridge_host_state_path: Option<PathBuf>,
) {
	// If we have an IP we can send a find request directly, otherwise we do a broadcast.
	let mion_identity_opt_res =
		find_identity_from_network(filter_ip, filter_mac, filter_name.as_deref()).await;
	let mion_identity_opt = match mion_identity_opt_res {
		Ok(opt) => opt,
		Err(cause) => {
			if use_json {
				error!(
					id = "bridgectl::get::failed_to_execute_broadcast",
					?cause,
					help = "Could not setup sockets to broadcast and search for all MIONs; perhaps another program is already using the single MION port? Trying to find MION from config file (will be less detailed).",
				);
			} else {
				error!(
					"\n{:?}",
					miette!(
						help = "Perhaps another program is already using the single MION port?",
						"Could not setup sockets to broadcast and search for a MION (trying to search config file, will be less detailed).",
					).wrap_err(cause),
				);
			}

			fallback_to_config_file(
				use_json,
				use_table,
				bridge_host_state_path,
				(filter_ip, filter_mac, filter_name),
				GET_FAILED_TO_SEARCH_FOR_DEVICE,
			)
			.await;
			// Async ! isn't stable and recognized :(
			unreachable!()
		}
	};

	let Some(identity) = mion_identity_opt else {
		if use_json {
			error!(
			  id = "bridgectl::get::get_failed_to_find_a_device",
			  filter.ip = ?filter_ip,
			  filter.mac = ?filter_mac,
			  filter.name = ?filter_name,
			  suggestions = valuable(&[
				  "Please ensure the CAT-DEV you're trying to find is powered on, and running.",
				  "Make sure you are on the same Local Network, Subnet, and VLAN as the CAT-DEV device.",
				  "If you're not on the same VLAN, Subnet you can use something like: <https://github.com/udp-redux/udp-broadcast-relay-redux> to forward between the subnets & vlans.",
				  "Ensure your filters line up with a single CAT-DEV device.",
			  ]),
			  help = "Is attempting to fallback to a config file (will be less detailed).",
			);

			fallback_to_config_file(
				use_json,
				use_table,
				bridge_host_state_path,
				(filter_ip, filter_mac, filter_name),
				GET_FAILED_TO_FIND_SPECIFIC_DEVICE,
			)
			.await;
			// Async ! isn't stable and recognized :(
			unreachable!()
		} else {
			error!(
    	  "\n{:?}",
				add_context_to(
					miette!(
						"Failed to find bridge that matched the series of filters -- falling back to config file (will be less detailed).",
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

			fallback_to_config_file(
				use_json,
				use_table,
				bridge_host_state_path,
				(filter_ip, filter_mac, filter_name),
				GET_FAILED_TO_FIND_SPECIFIC_DEVICE,
			)
			.await;
			// Async ! isn't stable and recognized :(
			unreachable!()
		}
	};

	print_detailed_bridge(use_json, use_table, &identity);
}

async fn fallback_to_config_file(
	use_json: bool,
	use_table: bool,
	bridge_host_state_path: Option<PathBuf>,
	filters: (Option<Ipv4Addr>, Option<MacAddress>, Option<String>),
	exit_code: i32,
) {
	let bridge_state = bridge_state_from_path(
		get_bridge_state_path(&bridge_host_state_path, use_json),
		use_json,
	)
	.await;

	if use_table {
		if use_json {
			info!(
				id = "bridgectl::get::fallback_table_print",
				line = FALLBACK_HEADER
			);
			info!(
				id = "bridgectl::get::fallback_table_print",
				line = FALLBACK_HEADER_LINE
			);
		} else {
			println!("{FALLBACK_HEADER}");
			println!("{FALLBACK_HEADER_LINE}");
		}
	}

	let mut found_any = false;
	for (bridge_name, (bridge_ip, is_default)) in bridge_state.list_bridges() {
		debug!(
			id = "bridgectl::get::is_fallback_match",
			potential_bridge.name = bridge_name,
			potential_bridge.ip = ?bridge_ip,
			potential_bridge.default = is_default,
			filters.ip = ?filters.0,
			filters.name = ?filters.1,
			filters.mac = ?filters.2,
		);
		let (is_match, missed_filters) = if bridge_ip.is_none()
			&& filters.2.as_deref().unwrap_or(bridge_name.as_str()) == bridge_name.as_str()
		{
			(
				true,
				if filters.0.is_some() && filters.1.is_some() {
					Some(vec![
						format!("ip:{:?}", filters.0),
						format!("mac:{:?}", filters.1),
					])
				} else if filters.1.is_some() {
					Some(vec![format!("mac:{:?}", filters.1)])
				} else {
					None
				},
			)
		} else if (filters.0.is_none()
			&& filters.2.as_deref().unwrap_or(bridge_name.as_str()) == bridge_name.as_str())
			|| (filters.0 == bridge_ip
				&& filters.2.as_deref().unwrap_or(bridge_name.as_str()) == bridge_name.as_str())
		{
			(
				true,
				if filters.1.is_some() {
					Some(vec![format!("mac:{:?}", filters.1)])
				} else {
					None
				},
			)
		} else {
			(false, None)
		};

		if is_match {
			found_any = true;
			let as_valuable = missed_filters.as_ref().map(|to_value| valuable(to_value));
			print_potential_bridge_match(
				use_json,
				use_table,
				&bridge_name,
				bridge_ip,
				is_default,
				as_valuable,
			);
		}
	}

	if found_any {
		std::process::exit(0);
	} else {
		print_no_fallback_found(use_json, filters.0, filters.1, filters.2.as_deref());
		std::process::exit(exit_code);
	}
}

async fn find_identity_from_network(
	filter_ip: Option<Ipv4Addr>,
	filter_mac: Option<MacAddress>,
	filter_name: Option<&str>,
) -> Result<Option<MionIdentity>, CatBridgeError> {
	// If we have an IP we can send a find request directly, otherwise we do a broadcast.
	if let Some(ip) = filter_ip {
		match find_mion(MIONFindBy::Ip(ip), true, None).await {
			Ok(Some(identity)) => {
				if (filter_mac.is_none() || filter_mac == Some(identity.mac_address()))
					&& (filter_name.is_none() || filter_name == Some(identity.name()))
				{
					Ok(Some(identity))
				} else {
					Ok(None)
				}
			}
			Ok(None) => Ok(None),
			Err(cause) => Err(cause),
		}
	} else {
		match discover_bridges(true).await {
			Ok(mut recv_channel) => {
				let mut value = None;

				while let Some(identity) = recv_channel.recv().await {
					if (filter_ip.is_none() || filter_ip == Some(identity.ip_address()))
						&& (filter_mac.is_none() || filter_mac == Some(identity.mac_address()))
						&& (filter_name.is_none() || filter_name == Some(identity.name()))
					{
						value = Some(identity);
						break;
					}
				}

				Ok(value)
			}
			Err(cause) => Err(cause),
		}
	}
}

fn print_detailed_bridge(use_json: bool, use_table: bool, bridge: &MionIdentity) {
	if use_table {
		if let Some((TermWidth(characters_wide), _)) = terminal_size() {
			if characters_wide < 150 {
				warn!(
          id = "bridgectl::get::terminal_may_be_small",
          width.expected=150,
          width.was=characters_wide,
          "!!! HEY! Your terminal width seems to be smaller than 150 characters! The table renders at ~150 characters, so we recommend making you terminal wider to see the table best !!!",
        );
			}
		}

		let rendered_name = get_padded_string(bridge.name(), 30);
		let rendered_ip = get_padded_string(bridge.ip_address(), 15);
		let rendered_mac = get_padded_string(bridge.mac_address(), 18);
		let rendered_fpga = get_padded_string(bridge.fpga_version(), 18);
		let rendered_fw = get_padded_string(bridge.firmware_version(), 16);
		let rendered_sdk = get_padded_string(
			bridge
				.detailed_sdk_version()
				.unwrap_or("<missing>  ".to_owned()),
			11,
		);
		let rendered_boot_mode = get_padded_string(
			bridge
				.detailed_boot_type()
				.map_or("<missing>".to_owned(), |bt| format!("{bt}")),
			9,
		);
		let rendered_power_status = get_padded_string(
			bridge
				.detailed_is_cafe_on()
				.map_or("<missing>", |is_on| if is_on { "ON" } else { "OFF" }),
			12,
		);
		let full_table_line = format!("{rendered_name} | {rendered_ip} | {rendered_mac} | {rendered_fpga} | {rendered_fw} | {rendered_sdk} | {rendered_boot_mode} | {rendered_power_status}");

		if use_json {
			info!(
				id = "bridgectl::get::found_requested_bridge_network_table",
				line = DETAILED_HEADER
			);
			info!(
				id = "bridgectl::get::found_requested_bridge_network_table",
				line = DETAILED_HEADER_LINE
			);
			info!(
				id = "bridgectl::get::found_requested_bridge_network_table",
				line = full_table_line,
				bridge = valuable(bridge)
			);
		} else {
			println!("{DETAILED_HEADER}");
			println!("{DETAILED_HEADER_LINE}");
			println!("{full_table_line}");
		}
	} else if use_json {
		info!(
			id = "bridgectl::get::found_requested_bridge_network",
			bridge = valuable(bridge),
			"Found the requested bridge on the network",
		);
	} else {
		info!(
		  bridge.name = bridge.name(),
		  bridge.ip_address = %bridge.ip_address(),
		  bridge.mac = %bridge.mac_address(),
		  bridge.fpga_version = %bridge.fpga_version(),
		  bridge.firmware_version = %bridge.firmware_version(),
		  bridge.sdk_version = bridge.detailed_sdk_version().unwrap_or("<missing data>".to_owned()),
		  bridge.boot_type = bridge.detailed_boot_type().map_or("<missing data>".to_owned(), |bt| format!("{bt}")),
		  bridge.is_cafe_on = bridge.detailed_is_cafe_on().map_or("<missing data>", |is_on| if is_on { "ON" } else { "OFF" }),
		  "Found the requested bridge on the network!",
		);
	}
}

fn print_potential_bridge_match(
	use_json: bool,
	use_table: bool,
	bridge_name: &str,
	bridge_ip: Option<Ipv4Addr>,
	is_default: bool,
	missed_filters: Option<ValuableValue<'_>>,
) {
	if use_table {
		let name = get_padded_string(bridge_name, 30);
		let ip = get_padded_string(
			bridge_ip.map_or("<missing info> ".to_owned(), |ip| format!("{ip}")),
			15,
		);
		let line = format!("{name} | {ip} | {is_default}");

		if use_json {
			info!(
			  id = "bridgectl::get::potential_bridge_match_table",
			  line,
			  bridge.name = bridge_name,
			  bridge.ip = ?bridge_ip,
			  bridge.is_default = is_default,
			  bridge.missed_filters = missed_filters.unwrap_or(valuable(&[""])),
			);
		} else {
			if let Some(mfilters) = missed_filters {
				debug!(
					missed_filters = mfilters,
					"couldn't fully confirm bridge matches, just a guess"
				);
			}
			println!("{line}");
		}
	} else if use_json {
		info!(
		  id = "bridgectl::get::potential_bridge_match",
		  bridge.name = bridge_name,
		  bridge.ip = ?bridge_ip,
		  bridge.is_default = is_default,
		  bridge.missed_filters = missed_filters.unwrap_or(valuable(&[""])),
		);
	} else {
		info!(
		  bridge.name = bridge_name,
		  bridge.ip = ?bridge_ip,
		  bridge.is_default = is_default,
		  bridge.missed_filters = missed_filters.unwrap_or(valuable(&[""])),
		  "Found potential bridge match!",
		);
	}
}

fn print_no_fallback_found(
	use_json: bool,
	filter_ip: Option<Ipv4Addr>,
	filter_mac: Option<MacAddress>,
	filter_name: Option<&str>,
) {
	if use_json {
		error!(
			id = "bridgectl::get::no_fallback_found",
			filters.ip = ?filter_ip,
			filters.mac = ?filter_mac,
			filters.name = ?filter_name,
			suggestions = valuable(&[
				"Please ensure the bridge filters actually apply to a single bridge.",
			]),
		);
	} else {
		error!(
			"\n{:?}",
			miette!(
				help = "Please ensure the bridge filters actually apply to a single bridge.",
				"Failed to find any bridge that matches your criteria in our configuration file.",
			),
		);
	}
}
