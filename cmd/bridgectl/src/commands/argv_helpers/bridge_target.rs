//! Utilities for commands that need to target just a single bridge.
//!
//! Besides a few discovery commands within [`bridgectl`], or commands not
//! related to bridges at all (like listing serial ports). Most of them need
//! to target just a singular bridge. They may just need an IP, they may just
//! need a name, they may need both pieces of information. Since error handling
//! is so verbose in order to try and get good logging messages & exit codes,
//! we wrap all of that common functionality in one place.
//!
//! This module will resolve the bridge to target in the following order of
//! precedence:
//!
//! 1. Always use a CLI flag if it's given. This can be any of:
//!   - `--default` (use the default bridge from a configuration file).
//!   - `--bridge-from-env` (use the bridge name/ip specified in the
//!      environment).
//!   - `-i`/`--ip`/`-m`/`--mac-address`/`-n`/`--name` filter flags through the cli.
//!   - the first positional argument (if no flags were given); note: this one
//!     may get a bit tricky because if there are other positional arguments
//!     the first one may not be a filter, but instead the argument after it.
//! 2. If no CLI flag is given, or the CLI flag doesn't seem to be a real
//!    bridge then we will fall back to using the environment variables
//!    provided by mochiato/cafex `BRIDGE_CURRENT_NAME`
//!    `BRIDGE_CURRENT_IP_ADDRESS`.
//! 3. Finally if there are no mochiato environment variables set, and the user
//!    didn't manually specify a bridge. We will attempt to load the
//!    configuration file for bridges, and fetch the default bridge.
//!
//! Only if all these sources return nothing will we bail out and say sorry
//! there's no bridge to target.
//!
//! I also want to mention that this module will attempt to avoid lookups
//! whenever possible. If you only need an IP Address, and all you've specified
//! is an ip, no lookups should occur. If you need an ip and a name, and you've
//! only specified an ip, we'll try looking at other sources (environment, and
//! configuration files) to find the name without doing a lookup. The local
//! sources will always be preferred. There are a few commands who may choose
//! to do an additional lookup (such as adding a bridge to the configuration
//! file to make sure it's not adding stale data), but the expectation should
//! be try to avoid doing a lookup whenever possible.
//!
//! When we are forced to do a lookup (because we're looking up information we
//! don't have locally), we will try to make our lookup as fast as possible,
//! but obviously the fastest will be when we don't have to do a lookup at all.

use crate::{
	commands::argv_helpers::{
		get_control_port, get_scan_timeout, lease_bridge_config, lease_bridge_config_optionally,
	},
	exit_codes::{
		ARGV_BRIDGE_CONFLICTING_ARGUMENTS, ARGV_COULD_NOT_GET_DEFAULT_BRIDGE,
		ARGV_COULD_NOT_SEARCH_FOR_BRIDGE, ARGV_NO_BRIDGE_ENV, ARGV_NO_BRIDGE_SPECIFIED,
		SHOULD_NEVER_HAPPEN_FAILURE,
	},
	knobs::{
		cli::TargetBridgeFlags,
		env::{BRIDGE_CURRENT_IP_ADDRESS, BRIDGE_CURRENT_NAME},
	},
};
use cat_dev::mion::{
	BridgeHostState,
	discovery::{MIONFindBy, find_mion},
	proto::control::MionIdentity,
};
use mac_address::MacAddress;
use std::{net::Ipv4Addr, time::Duration};
use tokio::sync::{RwLock, RwLockReadGuard};
use tracing::{debug, error, field::valuable, info, warn};

/// The name of the bridge we're actively targeting.
static TARGETED_BRIDGE_NAME: RwLock<Option<String>> = RwLock::const_new(None);
/// The IPv4 of the bridge we're actively targeting.
static TARGETED_BRIDGE_IP: RwLock<Option<Ipv4Addr>> = RwLock::const_new(None);
/// The MAC Address of the bridge we're actively targeting.
static TARGETED_BRIDGE_MAC: RwLock<Option<MacAddress>> = RwLock::const_new(None);

/// Perform 'targeting' of a single bridge, where we take all the various input
/// sources to find a single bridge to operate on.
///
/// This is called usually right before executing a command, and performs all
/// of the initial setup we can, given the flags we hold. This will not
/// actually execute a lookup, UNLESS you've specified something that could be
/// mac address of the bridge. This will force a lookup as we have neither the
/// name, nor the IP.
///
/// ## Exits
///
/// This will abnormally exit the program if we cannot target just a single
/// bridge.
pub async fn target_bridge(
	target_flags: TargetBridgeFlags,
	positional_argument: Option<&str>,
	first_arg_must_be_bridge: bool,
) -> bool {
	if target_flags.not_search_flag_specified() {
		target_default_or_mochiato(
			target_flags,
			positional_argument.is_some(),
			first_arg_must_be_bridge,
		)
		.await;
		return false;
	} else if target_flags.specified_bridge_search_flag() || positional_argument.is_some() {
		let return_value =
			target_search_flags(target_flags, positional_argument, first_arg_must_be_bridge).await;
		if let Some(used_first_arg) = return_value {
			return used_first_arg;
		}
	}

	if !try_to_load_from_env().await {
		info!(
			id = "bridgectl::argv::default_fallback",
			"No bridge specified in environment, and argument is missing or _may_ not be bridge name, trying to load default from configuration.",
		);

		if !try_to_load_default().await {
			error!(
				id = "bridgectl::argv::no_bridge_found",
				help = valuable(&[
					"You can specify a bridge with environment variables `BRIDGE_CURRENT_NAME`, `BRIDGE_CURRENT_IP_ADDRESS`.",
					"You can specify one with flags `--ip`, `--mac`, or `--name`.",
					"You can set a default bridge in your configuration file (see `bridgectl set-default`, or `bridgectl add --default`).",
				]),
				"Command needs to target a single bridge, and you didn't specify a single bridge to use.",
			);

			std::process::exit(ARGV_NO_BRIDGE_SPECIFIED);
		}
	}

	false
}

/// Get the name of the bridge actively being targeted.
pub async fn get_targeted_bridge_name() -> String {
	let read_lock = TARGETED_BRIDGE_NAME.read().await;
	if let Some(name) = read_lock.as_ref() {
		return name.to_owned();
	}
	std::mem::drop(read_lock);

	let mut write_lock = TARGETED_BRIDGE_NAME.write().await;
	// Someone could have beaten us to the punch.
	if let Some(name) = write_lock.as_ref() {
		return name.to_owned();
	}
	let ip_read_lock = TARGETED_BRIDGE_IP.read().await;
	let name = resolve_name_from_ip(*ip_read_lock.as_ref().expect("impossible")).await;
	_ = write_lock.insert(name.clone());
	name
}

/// Get the IP of the bridge actively being targeted.
pub async fn get_targeted_bridge_ip() -> Ipv4Addr {
	let read_lock = TARGETED_BRIDGE_IP.read().await;
	if let Some(ip) = read_lock.as_ref() {
		return *ip;
	}
	std::mem::drop(read_lock);

	let mut write_lock = TARGETED_BRIDGE_IP.write().await;
	if let Some(ip) = write_lock.as_ref() {
		return *ip;
	}
	let name_read_lock = TARGETED_BRIDGE_NAME.read().await;
	let ip = resolve_ip_from_name(name_read_lock.as_ref().expect("impossible").to_owned()).await;
	_ = write_lock.insert(ip);
	ip
}

/// Get the MAC Address of the bridge actively being targeted.
pub async fn get_targeted_bridge_mac() -> MacAddress {
	let read_lock = TARGETED_BRIDGE_MAC.read().await;
	if let Some(mac) = read_lock.as_ref() {
		return *mac;
	}
	std::mem::drop(read_lock);

	let mut write_lock = TARGETED_BRIDGE_MAC.write().await;
	if let Some(mac) = write_lock.as_ref() {
		return *mac;
	}
	let ip = get_targeted_bridge_ip().await;
	let mac = resolve_mac_from_ip(ip).await;
	_ = write_lock.insert(mac);
	mac
}

async fn resolve_mac_from_ip(mion_ip: Ipv4Addr) -> MacAddress {
	let port = get_control_port().await;
	let timeout = get_scan_timeout().await;

	match find_mion(MIONFindBy::Ip(mion_ip), false, Some(timeout), Some(port)).await {
		Ok(opt_mion_info) => get_mion_mac_from_scan_result(opt_mion_info, mion_ip, port, timeout),
		Err(cause) => {
			error!(
				id = "bridgectl::argv::failed_to_execute_search",
				?cause,
				filters.find_by = %MIONFindBy::Ip(mion_ip),
				scanning.port = %port,
				scanning.timeout_seconds = timeout.as_secs(),
				help = "Perhaps another program is already using the single MION port?",
				"Could not execute search for bridge...",
			);

			std::process::exit(ARGV_COULD_NOT_SEARCH_FOR_BRIDGE);
		}
	}
}

async fn resolve_name_from_ip(mion_ip: Ipv4Addr) -> String {
	// First check environment, they may have passed a flag, but the other info
	// was already in the environment.
	if let Some(ip_addr) = BRIDGE_CURRENT_IP_ADDRESS.as_ref()
		&& let Some(name) = BRIDGE_CURRENT_NAME.as_ref()
		&& mion_ip == *ip_addr
	{
		return name.to_owned();
	}
	// Next check the config file for a bridge defined with that ip.
	if let Some(bridge_conf) = lease_bridge_config_optionally().await {
		for (bridge_name, (opt_bridge_ip, _is_default)) in bridge_conf.list_bridges() {
			let Some(bip) = opt_bridge_ip else { continue };
			debug!(
				id = "bridgectl::argv::find_from_fallback_compare_ip",
				expected.ip = %mion_ip,
				found.ip = %bip,
				filtering.is_equal = bip == mion_ip,
				"checking bridge ip equality",
			);
			if bip == mion_ip {
				return bridge_name;
			}
		}
	} else {
		debug!(
			id = "bridgectl::argv::fallback_to_config",
			"Could not optionally lease bridge configuration..."
		);
	}

	// If we're able to do nothing else, we're relegated to do a search
	info!(
		id = "bridgectl::argv::have_bridge_ip_looking_up_name",
		help = "In order to prevent lookups being necessary, feel free to add this name/ip to your bridge configuration file (with `bridgectl add`).",
		"We have a Bridge IP, but can't find a bridge name in environment/configuration files.",
	);

	let port = get_control_port().await;
	let timeout = get_scan_timeout().await;

	match find_mion(MIONFindBy::Ip(mion_ip), false, Some(timeout), Some(port)).await {
		Ok(opt_mion_info) => {
			attempt_populate_mac_from_scan_result(opt_mion_info.as_ref());
			get_mion_name_from_scan_result(opt_mion_info, mion_ip, port, timeout)
		}
		Err(cause) => {
			error!(
				id = "bridgectl::argv::failed_to_execute_search",
				?cause,
				filters.find_by = %MIONFindBy::Ip(mion_ip),
				scanning.port = %port,
				scanning.timeout_seconds = timeout.as_secs(),
				help = "Perhaps another program is already using the single MION port?",
				"Could not execute search for bridge...",
			);

			std::process::exit(ARGV_COULD_NOT_SEARCH_FOR_BRIDGE);
		}
	}
}

async fn resolve_ip_from_name(mion_name: String) -> Ipv4Addr {
	// First check environment, they may have passed a flag, but the other info
	// was already in the environment.
	if let Some(ip_addr) = BRIDGE_CURRENT_IP_ADDRESS.as_ref()
		&& let Some(name) = BRIDGE_CURRENT_NAME.as_ref()
		&& name.as_str() == mion_name.as_str()
	{
		return ip_addr.to_owned();
	}
	// Next check the config file for a bridge defined with that ip.
	if let Some(bridge_conf) = lease_bridge_config_optionally().await {
		for (bridge_name, (opt_bridge_ip, _is_default)) in bridge_conf.list_bridges() {
			let Some(bip) = opt_bridge_ip else { continue };
			if bridge_name == mion_name {
				return bip;
			}
		}
	} else {
		debug!(
			id = "bridgectl::argv::fallback_to_config",
			"Could not optionally lease bridge configuration..."
		);
	}

	// If we're able to do nothing else, we're relegated to do a search
	info!(
		id = "bridgectl::argv::have_bridge_name_looking_up_ip",
		help = "In order to prevent lookups being necessary, feel free to add this name/ip to your bridge configuration file (with `bridgectl add`).",
		"We have a Bridge Name, but can't find a Bridge IP in environment/configuration files.",
	);

	let port = get_control_port().await;
	let timeout = get_scan_timeout().await;

	match find_mion(
		MIONFindBy::Name(mion_name.clone()),
		false,
		Some(timeout),
		Some(port),
	)
	.await
	{
		Ok(opt_mion_info) => {
			attempt_populate_mac_from_scan_result(opt_mion_info.as_ref());
			get_mion_ip_from_scan_result(opt_mion_info, mion_name, port, timeout)
		}
		Err(cause) => {
			error!(
				id = "bridgectl::argv::failed_to_execute_search",
				?cause,
				filters.find_by = %MIONFindBy::Name(mion_name),
				scanning.port = %port,
				scanning.timeout_seconds = timeout.as_secs(),
				help = "Perhaps another program is already using the single MION port?",
				"Could not execute search for bridge...",
			);

			std::process::exit(ARGV_COULD_NOT_SEARCH_FOR_BRIDGE);
		}
	}
}

/// Populates the targets mac address if we're already doing a search.
///
/// This helps prevent duplicate lookups/searches for the mac address.
fn attempt_populate_mac_from_scan_result(found_result: Option<&MionIdentity>) {
	let Some(result) = found_result else {
		return;
	};
	// If someone is already trying to write the mac, let them write it.
	let Ok(mut write_lock) = TARGETED_BRIDGE_MAC.try_write() else {
		return;
	};
	_ = write_lock.insert(result.mac_address());
}

/// Get the MAC address from a scan result.
fn get_mion_mac_from_scan_result(
	found_result: Option<MionIdentity>,
	mion_ip: Ipv4Addr,
	port: u16,
	timeout: Duration,
) -> MacAddress {
	if let Some(result) = found_result {
		return result.mac_address();
	}

	error!(
		id = "bridgectl::argv::search_returned_no_bridges",
		filters.find_by = %MIONFindBy::Ip(mion_ip),
		scanning.port = port,
		scanning.timeout_seconds = timeout.as_secs(),
		help = valuable(&[
			"Please ensure the CAT-DEV you're trying to find is powered on, and running.",
			"Make sure you are on the same Local Network, Subnet, and VLAN as the CAT-DEV device.",
			"If you're not on the same VLAN, Subnet you can use something like: <https://github.com/udp-redux/udp-broadcast-relay-redux> to forward between the subnets & vlans.",
			"Ensure your filters line up with a single CAT-DEV device.",
		]),
		"could not find a bridge with the filters on your network.",
	);

	std::process::exit(ARGV_NO_BRIDGE_SPECIFIED);
}

fn get_mion_ip_from_scan_result(
	found_result: Option<MionIdentity>,
	mion_name: String,
	port: u16,
	timeout: Duration,
) -> Ipv4Addr {
	if let Some(result) = found_result {
		return result.ip_address();
	}

	error!(
		id = "bridgectl::argv::search_returned_no_bridges",
		filters.find_by = %MIONFindBy::Name(mion_name),
		scanning.port = port,
		scanning.timeout_seconds = timeout.as_secs(),
		help = valuable(&[
			"Please ensure the CAT-DEV you're trying to find is powered on, and running.",
			"Make sure you are on the same Local Network, Subnet, and VLAN as the CAT-DEV device.",
			"If you're not on the same VLAN, Subnet you can use something like: <https://github.com/udp-redux/udp-broadcast-relay-redux> to forward between the subnets & vlans.",
			"Ensure your filters line up with a single CAT-DEV device.",
		]),
		"could not find a bridge with the filters on your network.",
	);

	std::process::exit(ARGV_NO_BRIDGE_SPECIFIED);
}

fn get_mion_name_from_scan_result(
	found_result: Option<MionIdentity>,
	mion_ip: Ipv4Addr,
	port: u16,
	timeout: Duration,
) -> String {
	if let Some(result) = found_result {
		return result.name().to_owned();
	}

	error!(
		id = "bridgectl::argv::search_returned_no_bridges",
		filters.find_by = %MIONFindBy::Ip(mion_ip),
		scanning.port = port,
		scanning.timeout_seconds = timeout.as_secs(),
		help = valuable(&[
			"Please ensure the CAT-DEV you're trying to find is powered on, and running.",
			"Make sure you are on the same Local Network, Subnet, and VLAN as the CAT-DEV device.",
			"If you're not on the same VLAN, Subnet you can use something like: <https://github.com/udp-redux/udp-broadcast-relay-redux> to forward between the subnets & vlans.",
			"Ensure your filters line up with a single CAT-DEV device.",
		]),
		"could not find a bridge with the filters on your network.",
	);

	std::process::exit(ARGV_NO_BRIDGE_SPECIFIED);
}

async fn target_search_flags(
	target_flags: TargetBridgeFlags,
	positional_argument: Option<&str>,
	exit_if_arg_invalid: bool,
) -> Option<bool> {
	let mut used_flags = false;
	let (find_by, extra_ip_filter, extra_name_filter, used_arg) = if target_flags
		.specified_bridge_search_flag()
	{
		used_flags = true;
		if target_flags.search_for_mac_raw().is_some() {
			if let Some(mac) = target_flags.search_for_mac() {
				(
					MIONFindBy::MacAddress(mac),
					target_flags.search_for_ip(),
					target_flags.search_for_name(),
					false,
				)
			} else {
				warn!(
					id = "bridgectl::argv::mac_flag_invalid",
					mac_flag = ?target_flags.search_for_mac_raw(),
					"Mac Flag is not a valid MAC Address, will not be used, and will exit if no other filters present."
				);

				if target_flags.search_for_ip().is_none()
					&& target_flags.search_for_name().is_none()
				{
					std::process::exit(ARGV_NO_BRIDGE_SPECIFIED);
				} else {
					if let Some(ip) = target_flags.search_for_ip() {
						let mut static_ip_opt = TARGETED_BRIDGE_IP.write().await;
						_ = static_ip_opt.insert(ip);
					}
					if let Some(name) = target_flags.search_for_name() {
						let mut static_name_opt = TARGETED_BRIDGE_NAME.write().await;
						_ = static_name_opt.insert(name.to_owned());
					}
					return Some(false);
				}
			}
		} else {
			if let Some(ip) = target_flags.search_for_ip() {
				let mut static_ip_opt = TARGETED_BRIDGE_IP.write().await;
				_ = static_ip_opt.insert(ip);
			}
			if let Some(name) = target_flags.search_for_name() {
				let mut static_name_opt = TARGETED_BRIDGE_NAME.write().await;
				_ = static_name_opt.insert(name.to_owned());
			}
			return Some(false);
		}
	} else {
		let Some(argument) = positional_argument else {
			if exit_if_arg_invalid {
				return None;
			}

			error!(
				"internal_error: target_search_flags() called when no search flags were specified",
			);
			std::process::exit(SHOULD_NEVER_HAPPEN_FAILURE);
		};

		match MIONFindBy::from(argument.to_owned()) {
			MIONFindBy::Ip(ip) => {
				let mut static_ip_opt = TARGETED_BRIDGE_IP.write().await;
				_ = static_ip_opt.insert(ip);
				return Some(true);
			}
			MIONFindBy::MacAddress(mac) => (MIONFindBy::MacAddress(mac), None, None, true),
			MIONFindBy::Name(name) => {
				let mut static_name_opt = TARGETED_BRIDGE_NAME.write().await;
				_ = static_name_opt.insert(name);
				return Some(true);
			}
		}
	};

	do_scan_initial(
		find_by,
		extra_ip_filter,
		extra_name_filter,
		used_arg,
		used_flags || exit_if_arg_invalid,
	)
	.await
}

async fn do_scan_initial(
	find_by: MIONFindBy,
	extra_ip_filter: Option<Ipv4Addr>,
	extra_name_filter: Option<&str>,
	used_arg: bool,
	should_exit: bool,
) -> Option<bool> {
	let port = get_control_port().await;
	let timeout = get_scan_timeout().await;

	info!(
		id = "bridgectl::argv::scanning_for_bridge",
		reason = "A MAC Address was specified, so we're scanning to confirm all information is correct.",
		filters.find_by = %find_by,
		filters.extra_ip_filter = ?extra_ip_filter,
		filters.extra_name_filter = ?extra_name_filter,
		scanning.port = %port,
		scanning.timeout_seconds = timeout.as_secs(),
		"looking up MION to target",
	);

	match find_mion(find_by.clone(), false, Some(timeout), Some(port)).await {
		Ok(opt_mion_info) => {
			process_found_mion(
				find_by,
				extra_ip_filter,
				extra_name_filter,
				opt_mion_info,
				port,
				timeout,
			)
			.await;
		}
		Err(cause) => {
			if should_exit {
				error!(
					id = "bridgectl::argv::failed_to_execute_search",
					?cause,
					filters.find_by = %find_by,
					filters.extra_ip_filter = ?extra_ip_filter,
					filters.extra_name_filter = ?extra_name_filter,
					scanning.port = %port,
					scanning.timeout_seconds = timeout.as_secs(),
					help = "Perhaps another program is already using the single MION port?",
					"Could not execute search for bridge...",
				);

				std::process::exit(ARGV_COULD_NOT_SEARCH_FOR_BRIDGE);
			}
		}
	}

	Some(used_arg)
}

async fn process_found_mion(
	find_by: MIONFindBy,
	extra_ip_filter: Option<Ipv4Addr>,
	extra_name_filter: Option<&str>,
	found_result: Option<MionIdentity>,
	scan_port: u16,
	scan_timeout: Duration,
) {
	if let Some(result) = found_result {
		let mut matches_all = true;

		if let Some(ip_filter) = extra_ip_filter
			&& result.ip_address() != ip_filter
		{
			matches_all = false;
		}
		if let Some(name_filter) = extra_name_filter
			&& result.name() != name_filter
		{
			matches_all = false;
		}

		if matches_all {
			let mut static_ip_opt = TARGETED_BRIDGE_IP.write().await;
			let mut static_name_opt = TARGETED_BRIDGE_NAME.write().await;
			let mut static_mac_opt = TARGETED_BRIDGE_MAC.write().await;
			_ = static_ip_opt.insert(result.ip_address());
			_ = static_mac_opt.insert(result.mac_address());
			_ = static_name_opt.insert(result.name().to_owned());
		}
	}

	error!(
		id = "bridgectl::argv::search_returned_no_bridges",
		filters.find_by = %find_by,
		filters.extra_ip_filter = ?extra_ip_filter,
		filters.extra_name_filter = ?extra_name_filter,
		scanning.port = scan_port,
		scanning.timeout_seconds = scan_timeout.as_secs(),
		help = valuable(&[
			"Please ensure the CAT-DEV you're trying to find is powered on, and running.",
			"Make sure you are on the same Local Network, Subnet, and VLAN as the CAT-DEV device.",
			"If you're not on the same VLAN, Subnet you can use something like: <https://github.com/udp-redux/udp-broadcast-relay-redux> to forward between the subnets & vlans.",
			"Ensure your filters line up with a single CAT-DEV device.",
		]),
		"could not find a bridge with the filters on your network.",
	);

	std::process::exit(ARGV_NO_BRIDGE_SPECIFIED);
}

async fn try_to_load_from_env() -> bool {
	if BRIDGE_CURRENT_NAME.is_none() && BRIDGE_CURRENT_IP_ADDRESS.is_none() {
		return false;
	}

	if let Some(name) = BRIDGE_CURRENT_NAME.as_ref() {
		let mut static_name_opt = TARGETED_BRIDGE_NAME.write().await;
		_ = static_name_opt.insert(name.clone());
	}
	if let Some(ip) = BRIDGE_CURRENT_IP_ADDRESS.as_ref() {
		let mut static_ip_opt = TARGETED_BRIDGE_IP.write().await;
		_ = static_ip_opt.insert(*ip);
	}
	true
}

async fn try_to_load_default() -> bool {
	let bridge_config = lease_bridge_config().await;

	if let Some((bridge_name, opt_ip)) = bridge_config.get_default_bridge() {
		{
			let mut static_name_opt = TARGETED_BRIDGE_NAME.write().await;
			_ = static_name_opt.insert(bridge_name);
		}

		if let Some(default_ip) = opt_ip {
			let mut static_ip_opt = TARGETED_BRIDGE_IP.write().await;
			_ = static_ip_opt.insert(default_ip);
		}

		true
	} else {
		false
	}
}

async fn target_default_or_mochiato(
	target_flags: TargetBridgeFlags,
	specified_first_arg: bool,
	first_arg_must_be_bridge: bool,
) {
	if target_flags.specified_bridge_search_flag()
		|| (first_arg_must_be_bridge && specified_first_arg)
	{
		error!(
			id = "bridgectl::argv::flags_conflict_on_device_search",
			targeting_flags = valuable(&target_flags),
			help = valuable(&[
				"If you want to fetch the default bridge all you need is to specify the `--default` flag, you don't need anything else.",
				"If you want to apply extra filtering to the output we recommend changing the log format to JSON, and doing the extra filtering using a tool like `jq`.",
			]),
			"We were told to not search and just use either the default bridge, or environment. While also being told to search for another device! Not sure which to do.",
		);

		std::process::exit(ARGV_BRIDGE_CONFLICTING_ARGUMENTS);
	} else if target_flags.target_default() {
		load_bridge_from_default(lease_bridge_config().await).await;
	} else {
		load_from_mochiato().await;
	}
}

async fn load_bridge_from_default(config: RwLockReadGuard<'_, BridgeHostState>) {
	let Some((bridge_name, opt_ip)) = config.get_default_bridge() else {
		error!(
			id = "bridgectl::argv::no_default_bridge",
			host_state_path = %config.get_path().display(),
			help = valuable(&[
				"Please double check the configuration file located at the path specified, and ensure `BRIDGE_DEFAULT_NAME` is set to a real bridge name.",
				"If the bridge isn't set as the default you can use `bridge add --default <name> <ip>`, or `bridge set-default <'name' or 'ip'>`.",
			]),
			"No default bridge present in configuration file.",
		);

		std::process::exit(ARGV_COULD_NOT_GET_DEFAULT_BRIDGE);
	};

	{
		let mut static_name_opt = TARGETED_BRIDGE_NAME.write().await;
		_ = static_name_opt.insert(bridge_name);
	}

	if let Some(default_ip) = opt_ip {
		let mut static_ip_opt = TARGETED_BRIDGE_IP.write().await;
		_ = static_ip_opt.insert(default_ip);
	}
}

async fn load_from_mochiato() {
	if BRIDGE_CURRENT_NAME.is_none() && BRIDGE_CURRENT_IP_ADDRESS.is_none() {
		error!(
			id = "bridgectl::argv::no_bridge_environment",
			help =
				"You can use `cafex`/`mochiato` shells to set these environment variables for you.",
			"We were told to use the bridge from the environment, but `BRIDGE_CURRENT_NAME` & `BRIDGE_CURRENT_IP_ADDRESS` are not set!",
		);

		std::process::exit(ARGV_NO_BRIDGE_ENV);
	}

	if let Some(name) = BRIDGE_CURRENT_NAME.as_ref() {
		let mut static_name_opt = TARGETED_BRIDGE_NAME.write().await;
		_ = static_name_opt.insert(name.clone());
	}
	if let Some(ip) = BRIDGE_CURRENT_IP_ADDRESS.as_ref() {
		let mut static_ip_opt = TARGETED_BRIDGE_IP.write().await;
		_ = static_ip_opt.insert(*ip);
	}
}
