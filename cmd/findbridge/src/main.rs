#![allow(
	// I've always disliked this rule, most of the time imports are used WITHOUT
	// the module name, and the module name is only used in the top level import.
	//
	// Where this becomes significantly more helpful to read as it's out of
	// context.
	clippy::module_name_repetitions,
)]

pub mod knobs;
mod output;

use crate::output::{
	create_interface_logging_hook, print_bridge, print_bridge_header,
	print_verbose_search_suggestions,
};
use cat_dev::mion::discovery::{
	discover_bridges_with_logging_hooks, find_mion_with_logging_hooks, get_all_broadcast_addresses,
	MIONFindBy,
};
use knobs::cli::CliOpts;
use mac_address::MacAddress;
use std::time::Duration;
use tokio::{runtime::Runtime, time::sleep};

/// The single "error" exit code we use when findbridge error's.
const ERROR_EXIT_CODE: i32 = -1;

fn main() {
	let mut opts = CliOpts::from(std::env::args().skip(1));
	let mut exit_successfully = false;
	if opts.verbose {
		println!("findbridge v5.1 - Copyright (c) 2011 Nintendo Co., Ltd.");
	}

	if opts.help {
		CliOpts::print_help();
	} else if let Some(specific_item) = opts.find_specific.take() {
		let Ok(runtime) = Runtime::new() else {
			println!("ERROR: 164: Could not create enum thread!");
			return;
		};
		exit_successfully = runtime.block_on(find_one(specific_item, &opts));
	} else if opts.all {
		let Ok(runtime) = Runtime::new() else {
			println!("ERROR: 164: Could not create enum thread!");
			return;
		};
		runtime.block_on(scan_all(&opts));
	} else {
		// We just do nothing, and exit cleanly.
	}

	if !exit_successfully {
		std::process::exit(ERROR_EXIT_CODE);
	}
}

async fn find_one(find_arg: String, opts: &CliOpts) -> bool {
	let mut found_one = false;
	let mut is_finding_by_mac = false;

	let search_type = if opts.is_forced_mac {
		let Ok(mac) = MacAddress::try_from(find_arg.as_str()) else {
			// When the mac address isn't valid the original findbridge still
			// tries to scan anyway.
			fake_logging_hooks_for_scan(opts.verbose);
			// We print an extra newline if not using verbose to match.
			if !opts.verbose {
				println!();
			}
			println!("ERROR : Could not find bridge with MAC Address {find_arg}");
			if opts.verbose {
				print_verbose_search_suggestions();
			}
			return found_one;
		};

		is_finding_by_mac = true;
		MIONFindBy::MacAddress(mac)
	} else {
		MIONFindBy::from_name_or_ip(find_arg)
	};

	// For some reason when querying by name, it NEVER fetches detailed info.
	//
	// I don't know why.
	let force_non_detailed = matches!(search_type, MIONFindBy::Name(_));
	let Ok(bridge_opt) = find_mion_with_logging_hooks(
		search_type.clone(),
		if force_non_detailed {
			false
		} else {
			opts.detail
		},
		Some(Duration::from_secs(3)),
		create_interface_logging_hook(opts.verbose),
	)
	.await
	else {
		// Error 164 is "MAX_THRDS_REACHED" which is one of the two error conditions.
		//
		// Since socket creation didn't have an explicit error code, even though it
		// is possible to fail -- this is the error code we can use that the
		// original tool would create.
		println!("ERROR: 164: Could not create enum thread!");
		return false;
	};
	if let Some(bridge) = bridge_opt {
		found_one = true;
		print_bridge(&bridge, opts.detail, opts.list, false);
	}

	if !found_one {
		if is_finding_by_mac {
			println!("ERROR : Could not find bridge with MAC Address {search_type}");
		} else {
			println!("ERROR 258: Host bridge {search_type} not found.");
		}
	}

	found_one
}

async fn scan_all(opts: &CliOpts) {
	let Ok(mut recv_channel) = discover_bridges_with_logging_hooks(
		opts.detail,
		create_interface_logging_hook(opts.verbose),
	)
	.await
	else {
		// Error 164 is "MAX_THRDS_REACHED" which is one of the two error conditions.
		//
		// Since socket creation didn't have an explicit error code, even though it
		// is possible to fail -- this is the error code we can use that the
		// original tool would create.
		println!("ERROR: 164: Could not create enum thread!");
		return;
	};

	let mut found_bridges = Vec::with_capacity(0);
	loop {
		tokio::select! {
			opt = recv_channel.recv() => {
				let Some(identity) = opt else {
					// Opposite channel got closed.
					if found_bridges.is_empty() {
						print_bridge_header(opts.detail, opts.list);
					}
					break;
				};

				if !found_bridges.contains(&identity) {
					if found_bridges.is_empty() {
						print_bridge_header(opts.detail, opts.list);
					}
					print_bridge(&identity, opts.detail, opts.list, found_bridges.is_empty());
					found_bridges.push(identity);
				}
			}
			() = sleep(Duration::from_secs(3)) => {
				break;
			}
		}
	}

	if found_bridges.is_empty() {
		println!("\nERROR : No bridges found.");
		if opts.verbose {
			print_verbose_search_suggestions();
		}
	}
}

/// Fake calling logging hooks when we don't actually need to scan.
///
/// We actually don't always do a scan, because it's incredibly ineffecient to
/// do so. However, we still need to create logs for interfaces to match the
/// output 1:1.
fn fake_logging_hooks_for_scan(is_verbose: bool) {
	let interface_hook = create_interface_logging_hook(is_verbose);

	if let Ok(broadcast_addresses) = get_all_broadcast_addresses() {
		for (addr, _ipv4) in broadcast_addresses {
			interface_hook(&addr);
		}
	}

	if is_verbose {
		println!();
	}
}
