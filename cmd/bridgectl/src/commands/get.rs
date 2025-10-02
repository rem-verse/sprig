//! Handles fetching the information for just one particular bridge.

use crate::{
	commands::argv_helpers::{
		get_control_port, get_padded_string, get_scan_timeout, get_targeted_bridge_ip,
		get_targeted_bridge_name, lease_bridge_config,
	},
	exit_codes::{GET_FAILED_TO_FIND_SPECIFIC_DEVICE, GET_FAILED_TO_SEARCH_FOR_DEVICE},
};
use cat_dev::mion::{
	discovery::{MIONFindBy, find_mion},
	proto::control::MionIdentity,
};
use rm_lisa::display::SuperConsole;
use std::{
	io::{Stderr, Stdout},
	net::Ipv4Addr,
};
use tracing::{debug, error, field::valuable, info, warn};

const FALLBACK_HEADER: &str = "Bridge Name                    | IP Address      | Is Default";
const FALLBACK_HEADER_LINE: &str = "-------------------------------------------------------------";

const DETAILED_HEADER: &str = "Bridge Name                    | IP Address      | MAC Address        | FPGA image version | Firmware Version | SDK Version | Boot Mode | Power Status";
const DETAILED_HEADER_LINE: &str = "------------------------------------------------------------------------------------------------------------------------------------------------------";

/// Actual command handler for the `get` command.
pub async fn handle_get(use_table: bool) {
	let bridge_ip = get_targeted_bridge_ip().await;
	info!(
		id = "bridgectl::get::looking_up_detailed_bridge_info",
		%bridge_ip,
		"looking up detailed bridge information...",
	);

	let mion_identity_opt = match find_mion(
		MIONFindBy::Ip(bridge_ip),
		true,
		Some(get_scan_timeout().await),
		Some(get_control_port().await),
	)
	.await
	{
		Ok(opt) => opt,
		Err(cause) => {
			error!(
				id = "bridgectl::get::failed_to_execute_broadcast",
				?cause,
				help = "Could not setup sockets to broadcast and search for all MIONs; perhaps another program is already using the single MION port? Trying to find MION from config file (will be less detailed).",
			);

			fallback_to_config_file(use_table, GET_FAILED_TO_SEARCH_FOR_DEVICE).await;
			// Async ! isn't stable and recognized :(
			unreachable!()
		}
	};

	let Some(identity) = mion_identity_opt else {
		error!(
			id = "bridgectl::get::get_failed_to_find_a_device",
			suggestions = valuable(&[
				"Please ensure the CAT-DEV you're trying to find is powered on, and running.",
				"Make sure you are on the same Local Network, Subnet, and VLAN as the CAT-DEV device.",
				"If you're not on the same VLAN, Subnet you can use something like: <https://github.com/udp-redux/udp-broadcast-relay-redux> to forward between the subnets & vlans.",
				"Ensure your filters line up with a single CAT-DEV device.",
			]),
			help = "Is attempting to fallback to a config file (will be less detailed).",
		);

		fallback_to_config_file(use_table, GET_FAILED_TO_FIND_SPECIFIC_DEVICE).await;
		// Async ! isn't stable and recognized :(
		unreachable!()
	};

	print_detailed_bridge(use_table, &identity);
}

async fn fallback_to_config_file(use_table: bool, exit_code: i32) {
	let bridge_name = get_targeted_bridge_name().await;
	let bridge_ip = get_targeted_bridge_ip().await;
	let bridge_state = lease_bridge_config().await;

	if use_table {
		info!(id = "bridgectl::get::fallback_table_print", FALLBACK_HEADER);
		info!(
			id = "bridgectl::get::fallback_table_print",
			FALLBACK_HEADER_LINE
		);
	}

	let mut found_any = false;
	for (conf_bridge_name, (_opt_bridge_ip, is_default)) in bridge_state.list_bridges() {
		debug!(
			id = "bridgectl::get::is_fallback_match",
			potential_bridge.name = bridge_name,
			"fallback match check",
		);

		if conf_bridge_name == bridge_name {
			found_any = true;
			print_potential_bridge_match(use_table, &bridge_name, bridge_ip, is_default);
		}
	}

	if found_any {
		std::process::exit(0);
	} else {
		print_no_fallback_found();
		std::process::exit(exit_code);
	}
}

fn print_detailed_bridge(use_table: bool, bridge: &MionIdentity) {
	if use_table {
		if let Some(characters_wide) = SuperConsole::<Stdout, Stderr>::terminal_width()
			&& characters_wide < 214
		{
			warn!(
				id = "bridgectl::get::terminal_may_be_small",
				width.expected = 214,
				width.was = characters_wide,
				"!!! HEY! Your terminal width seems to be smaller than 214 characters! The table renders at ~150 characters, so we recommend making you terminal wider to see the table best !!!",
			);
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
		let full_table_line = format!(
			"{rendered_name} | {rendered_ip} | {rendered_mac} | {rendered_fpga} | {rendered_fw} | {rendered_sdk} | {rendered_boot_mode} | {rendered_power_status}"
		);

		info!(
			id = "bridgectl::get::found_requested_bridge_network_table",
			DETAILED_HEADER,
		);
		info!(
			id = "bridgectl::get::found_requested_bridge_network_table",
			DETAILED_HEADER_LINE,
		);
		info!(
			id = "bridgectl::get::found_requested_bridge_network_table",
			bridge = valuable(bridge),
			full_table_line,
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
	use_table: bool,
	bridge_name: &str,
	bridge_ip: Ipv4Addr,
	is_default: bool,
) {
	if use_table {
		let name = get_padded_string(bridge_name, 30);
		let ip = get_padded_string(bridge_ip, 15);
		let line = format!("{name} | {ip} | {is_default}");

		info!(
			id = "bridgectl::get::potential_bridge_match_table",
			bridge.name = bridge_name,
			bridge.ip = ?bridge_ip,
			bridge.is_default = is_default,
			line,
		);
	} else {
		info!(
			bridge.name = bridge_name,
			bridge.ip = ?bridge_ip,
			bridge.is_default = is_default,
			"Found potential bridge match!",
		);
	}
}

fn print_no_fallback_found() {
	error!(
		id = "bridgectl::get::no_fallback_found",
		suggestions =
			valuable(&["Please ensure the bridge filters actually apply to a single bridge."]),
	);
}
