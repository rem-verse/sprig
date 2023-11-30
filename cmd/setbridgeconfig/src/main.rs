#![allow(
	// I've always disliked this rule, most of the time imports are used WITHOUT
	// the module name, and the module name is only used in the top level import.
	//
	// Where this becomes significantly more helpful to read as it's out of
	// context.
	clippy::module_name_repetitions,
)]

pub mod knobs;

use crate::knobs::cli::CliOpts;
use cat_dev::BridgeHostState;
use std::net::Ipv4Addr;
use tokio::runtime::Runtime;

fn main() {
	let opts = CliOpts::from(std::env::args().skip(1));
	// Yes this is the real behavior, idk why.
	if opts.arg_count < 2 || ((opts.default || opts.delete || opts.protect) && opts.arg_count <= 2)
	{
		CliOpts::print_help();
		std::process::exit(-1);
	}

	if opts.delete {
		let Some(bridge_name) = opts.bridge_name else {
			// Nothing actually happens in this case, it just ends up returning an error.
			std::process::exit(-1);
		};
		let Some((mut bridge_state, runtime)) = get_bridge_state_and_runtime() else {
			std::process::exit(-1);
		};
		bridge_state.remove_bridge(&bridge_name);
		if runtime.block_on(bridge_state.write_to_disk()).is_err() {
			// This is an error happening for "System cannot write to the specified
			// device" which is probably pretty close to what happened.
			println!("\nERROR 29: Could not delete entry from the INI file");
			std::process::exit(-1);
		}
	} else {
		// We check for a valid IP FIRST, then we actually do stuff.
		let bridge_ip_opt = opts
			.bridge_ipaddr
			.map(|ip_str| match ip_str.parse::<Ipv4Addr>() {
				Ok(ip) => ip,
				Err(_cause) => {
					println!("ERROR : Please specify a valid IPv4 address");
					std::process::exit(-1);
				}
			});

		// Next we ensure we have a name, get the runtime, and ensure we have an
		// IP since we're adding and need one.
		let Some(bridge_name) = opts.bridge_name else {
			println!("\nERROR : Please specify a name");
			std::process::exit(-1);
		};
		let Some((mut bridge_state, runtime)) = get_bridge_state_and_runtime() else {
			std::process::exit(-1);
		};
		let Some(bridge_ip) = bridge_ip_opt else {
			println!("\nPlease specify a Name AND an IP address in order to add an entry.");
			std::process::exit(-1);
		};

		protect_bridge(opts.protect, &bridge_state, &bridge_name, bridge_ip);
		if bridge_state.upsert_bridge(&bridge_name, bridge_ip).is_err() {
			// The actual setdefaultbridge doesn't do any validation, just pretend we
			// have a write error.
			println!("\nERROR 29: Could not write to the INI file");
			std::process::exit(-1);
		}
		if opts.default {
			// This should never error, we've upsert'd it beforehand successfully.
			_ = bridge_state.set_default_bridge(&bridge_name);
		}
		if runtime.block_on(bridge_state.write_to_disk()).is_err() {
			println!("\nERROR 29: Could not write to the INI file");
			std::process::exit(-1);
		}
	}
}

fn get_bridge_state_and_runtime() -> Option<(BridgeHostState, Runtime)> {
	let Some(host_path) = BridgeHostState::get_default_host_path() else {
		println!(
			"\nERROR 203: Could not retrieve install path. Is Host Bridge Software installed?"
		);
		return None;
	};
	let Ok(runtime) = Runtime::new() else {
		// We only use this runtime for reading the file -- so we just show the
		// same error as if we couldn't read the file.
		println!("\nERROR 30: Could not retrieve install path. Is Host Bridge Software installed?");
		return None;
	};
	let Ok(host_env) = runtime.block_on(BridgeHostState::load_explicit_path(host_path)) else {
		// This is the error code for cannot read from specified device. This is
		// probably the closest we can get to a real error.
		println!("\nERROR 30: Could not retrieve install path. Is Host Bridge Software installed?");
		return None;
	};

	Some((host_env, runtime))
}

fn protect_bridge(
	is_protecting: bool,
	state: &BridgeHostState,
	bridge_name: &str,
	bridge_ip: Ipv4Addr,
) {
	if let Some((existing_bridge_ip, _is_default)) = state.get_bridge(bridge_name) {
		if existing_bridge_ip != Some(bridge_ip) {
			if is_protecting {
				println!("\nERROR : The specified IP address {bridge_ip} conflicts with an existing entry !");
				std::process::exit(-1);
			} else if let Some(ex_bridge_ip) = existing_bridge_ip {
				println!("\nWARNING : IP address for bridge \"{bridge_name}\" updated from {ex_bridge_ip} to {bridge_ip}");
			} else {
				println!("\nWARNING : IP address for bridge \"{bridge_name}\" updated from to {bridge_ip}");
			}
		}
	}
}
