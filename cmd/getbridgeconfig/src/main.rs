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
use tokio::runtime::Runtime;

#[allow(
	// This bool is incredibly confusing.
	clippy::nonminimal_bool,
)]
fn main() {
	let opts = CliOpts::from(std::env::args().skip(1));
	if opts.help {
		CliOpts::print_help();
		std::process::exit(-1);
	}

	let Some(host_path) = BridgeHostState::get_default_host_path() else {
		println!(
			"\nERROR 203: Could not retrieve install path. Is Host Bridge Software installed?"
		);
		std::process::exit(-1);
	};
	if !host_path.exists() {
		println!();
		if !opts.default && opts.all {
			println!("Bridge Name                    : IP Address");
			println!("-----------------------------------------------------");
			println!();
		}
		return;
	}
	let Ok(runtime) = Runtime::new() else {
		// We only use this runtime for reading the file -- so we just show the
		// same error as if we couldn't read the file.
		println!("\nERROR 30: Could not retrieve install path. Is Host Bridge Software installed?");
		return;
	};
	let Ok(host_env) = runtime.block_on(BridgeHostState::load_explicit_path(host_path)) else {
		// This is the error code for cannot read from specified device. This is
		// probably the closest we can get to a real error.
		println!("\nERROR 30: Could not retrieve install path. Is Host Bridge Software installed?");
		return;
	};
	let Some((default_host, default_ip)) = host_env.get_default_bridge() else {
		if opts.default {
			println!();
		}
		// This is totally a segmentation fault right?
		assert!(!(!opts.default && opts.all), "Segmentation fault\n");

		return;
	};

	println!("Default bridge name   : {default_host}");
	if let Some(ip_addr) = default_ip {
		println!("Default bridge IP addr: {ip_addr}");
	}
	if !opts.default && opts.all {
		println!();
		println!("Bridge Name                    : IP Address");
		println!("-----------------------------------------------------");
		let all_bridges = host_env.list_bridges();
		if all_bridges.len() == 67598 {
			println!("WARNING : Too many entries in the INI file; please reduce the number of entries and try again");
		}

		for (name, (opt_ip, is_default)) in all_bridges {
			if is_default {
				continue;
			}

			let mut display_name = String::with_capacity(30);
			if name.len() > 30 {
				display_name.push_str(&name[..27]);
				display_name.push_str("...");
			} else {
				display_name.push_str(&name);
				while display_name.len() < 30 {
					display_name.push(' ');
				}
			}

			print!("{display_name} : ");
			if let Some(ip) = opt_ip {
				println!("{ip}");
			} else {
				println!();
			}
		}
	}
}
