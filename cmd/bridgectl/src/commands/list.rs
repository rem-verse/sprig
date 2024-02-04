//! Handling listing all the bridges on the network, or cached bridges.

use crate::{
	exit_codes::LIST_COULD_NOT_SEARCH,
	utils::{add_context_to, bridge_state_from_path, get_bridge_state_path},
};
use cat_dev::{
	mion::{discovery::discover_bridges, proto::control::MionIdentity},
	BridgeHostState,
};
use miette::miette;
use std::{fmt::Display, path::PathBuf};
use terminal_size::{terminal_size, Width as TermWidth};
use tokio::time::{sleep, Duration};
use tracing::{error, field::valuable, info, warn};

const TABLE_TRACING_ID: &str = "bridgectl::list::table_output_line";
const DEFAULT_EARLY_TIMEOUT: u64 = 3;

/// Handle the actual `ls` command.
pub async fn handle_list(
	use_json: bool,
	use_cache: bool,
	output_as_table: bool,
	scan_timeout: Option<u64>,
	argv_host_state_path: Option<PathBuf>,
) {
	if use_cache {
		list_from_cache(
			use_json,
			output_as_table,
			&bridge_state_from_path(
				get_bridge_state_path(&argv_host_state_path, use_json),
				use_json,
			)
			.await,
		);
	} else {
		list_from_network(use_json, output_as_table, scan_timeout).await;
	}
}

/// List all of the devices that are actively on the network.
async fn list_from_network(use_json: bool, use_table: bool, scan_timeout: Option<u64>) {
	const TABLE_HEADER: &str =      "Bridge Name                    | IP Address      | MAC Address        | FPGA image version | Firmware Version | SDK Version | Boot Mode | Power Status";
	const TABLE_HEADER_LINE: &str = "------------------------------------------------------------------------------------------------------------------------------------------------------";

	let mut recv_channel = match discover_bridges(true).await {
		Ok(channel) => channel,
		Err(cause) => {
			if use_json {
				error!(
					id = "bridgectl::list::failed_to_execute_broadcast",
					cause = ?cause,
					help = "Could not setup sockets to broadcast and search for all MIONs; perhaps another program is already using the single MION port?",
				);
			} else {
				error!(
					"\n{:?}",
					miette!(
						help = "Perhaps another program is already using the single MION port?",
						"Could not setup sockets to broadcast and search for all MIONs",
					)
					.wrap_err(cause),
				);
			}

			std::process::exit(LIST_COULD_NOT_SEARCH);
		}
	};

	if use_table {
		if let Some((TermWidth(characters_wide), _)) = terminal_size() {
			if characters_wide < 150 {
				warn!(
          id = "bridgectl::list::terminal_may_be_small",
          width.expected=150,
          width.was=characters_wide,
          "!!! HEY! Your terminal width seems to be smaller than 150 characters! The table renders at ~150 characters, so we recommend making you terminal wider to see the table best !!!",
        );
			}
		}

		if use_json {
			info!(id = TABLE_TRACING_ID, line = TABLE_HEADER);
			info!(id = TABLE_TRACING_ID, line = TABLE_HEADER_LINE);
		} else {
			println!("{TABLE_HEADER}");
			println!("{TABLE_HEADER_LINE}");
		}
	}

	let mut found_bridges = Vec::new();
	let mut had_early_timeout = false;
	loop {
		tokio::select! {
		  opt_bridge = recv_channel.recv() => {
				let Some(bridge) = opt_bridge else {
				  break;
				};

				if !found_bridges.contains(&bridge) {
					print_detailed_bridge(&bridge, use_json, use_table);
				}
				found_bridges.push(bridge);
		  }
		  () = sleep(Duration::from_secs(scan_timeout.unwrap_or(DEFAULT_EARLY_TIMEOUT))) => {
				had_early_timeout = true;
			  break;
		  }
		}
	}

	if found_bridges.is_empty() {
		print_no_bridge_found_warning(use_json, had_early_timeout, scan_timeout);
	}
}

fn print_detailed_bridge(bridge: &MionIdentity, use_json: bool, use_table: bool) {
	if use_table {
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
				id = TABLE_TRACING_ID,
				line = full_table_line,
				bridge = valuable(&bridge)
			);
		} else {
			println!("{full_table_line}");
		}
	} else if use_json {
		info!(
			id = "bridgectl::list::discovered_bridge_over_network",
			bridge = valuable(&bridge),
			"Found a bridge on the network",
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
		  "Found a bridge on the network!",
		);
	}
}

fn print_no_bridge_found_warning(use_json: bool, was_early_exit: bool, early_timeout: Option<u64>) {
	if use_json {
		let mut suggestions = vec![
			"Please ensure the CAT-DEV is powered on, and running.".to_owned(),
			"Make sure you are on the same Local Network, Subnet, and VLAN as the CAT-DEV device.".to_owned(),
			"If you're not on the same VLAN, Subnet you can use something like: <https://github.com/udp-redux/udp-broadcast-relay-redux> to forward between the subnets & vlans.".to_owned(),
		];
		if was_early_exit {
			suggestions.push(format!(
        "We stopped searching early, you can raise the time before we give up searching you can use the CLI Argument with `--early-timeout-seconds <seconds>`, we timed out at {} seconds.",
        early_timeout.unwrap_or(DEFAULT_EARLY_TIMEOUT),
      ));
		}

		warn!(
			id = "bridgectl::list::failed_to_find_any_device",
			suggestions = valuable(&suggestions),
			"Could not find any bridges while broadcasting"
		);
	} else {
		let mut suggestions = vec![
			miette!("Please ensure the CAT-DEV is powered on, and running."),
			miette!("Make sure you are on the same Local Network, Subnet, and VLAN as the CAT-DEV device."),
			miette!("If you're not on the same VLAN, Subnet you can use something like: <https://github.com/udp-redux/udp-broadcast-relay-redux> to forward between the two VLANs/Subnets."),
		];
		if was_early_exit {
			suggestions.push(miette!(format!(
        "We stopped searching early (at {}s), you can raise the time we stop searching early with the CLI Flag: `--early-timeout-seconds`.",
				early_timeout.unwrap_or(DEFAULT_EARLY_TIMEOUT),
      )));
		}

		warn!(
			"\n{:?}",
			add_context_to(
				miette!("Could not find a bridge by the name, or potentially an ip."),
				suggestions.into_iter(),
			),
		);
	}
}

/// List the bridges from a cache rather than doing a full broadcast.
///
/// NOTE: There WILL be less information available here, frankly just because
/// we don't cache all the details about the host. In fact the `bridge_env.ini`
/// only stores the name of the bridge and the ip address. So that's all the
/// info you will get.
fn list_from_cache(use_json: bool, use_table: bool, host_state: &BridgeHostState) {
	const TABLE_HEADER: &str = "Bridge Name                    | IP Address      | Is Default";
	const TABLE_HEADER_LINE: &str = "-------------------------------------------------------------";

	let bridges = host_state.list_bridges();
	if bridges.is_empty() {
		if use_json {
			info!(id = "bridgectl::list::cache_has_no_bridges", ?host_state);
		} else {
			info!(
				host_state.path=%host_state.get_path().display(),
				"Couldn't find any cached bridges (perhaps try without the cached flag, or validate the path?). You can use `bridgectl add` to add bridges to your host state.",
			);
		}
		return;
	}

	if use_table {
		if use_json {
			info!(id = TABLE_TRACING_ID, line = TABLE_HEADER);
			info!(id = TABLE_TRACING_ID, line = TABLE_HEADER_LINE);
		} else {
			println!("{TABLE_HEADER}");
			println!("{TABLE_HEADER_LINE}");
		}
	}

	for (bridge_name, (bridge_ip, is_default)) in bridges {
		if use_table {
			let table_line = {
				let name = get_padded_string(&bridge_name, 30);
				let ip = get_padded_string(
					bridge_ip.map_or("<missing info> ".to_owned(), |ip| format!("{ip}")),
					15,
				);

				format!("{name} | {ip} | {is_default}")
			};
			if use_json {
				info!(
				  id = TABLE_TRACING_ID,
				  line = table_line,
				  bridge.ip = ?bridge_ip,
				  bridge.is_default = is_default,
				  bridge.name = bridge_name,
				);
			} else {
				println!("{table_line}");
			}
		} else if use_json {
			info!(id = "bridgectl::list::bridge_info", bridge.ip = ?bridge_ip, bridge.is_default = is_default, bridge.name = bridge_name);
		} else {
			info!(
				bridge.ip_address =
					bridge_ip.map_or("<missing info>".to_owned(), |ip| format!("{ip}")),
				bridge.is_default = is_default,
				bridge.name = bridge_name,
				"Located a bridge.",
			);
		}
	}
}

pub(crate) fn get_padded_string(ty: impl Display, max_length: usize) -> String {
	let mut to_return = String::with_capacity(max_length);
	let as_display = format!("{ty}");
	if as_display.len() > max_length {
		to_return.push_str(&as_display[..(max_length - 3)]);
		to_return.push_str("...");
	} else {
		to_return = as_display;
		while to_return.len() < max_length {
			to_return.push(' ');
		}
	}
	to_return
}
