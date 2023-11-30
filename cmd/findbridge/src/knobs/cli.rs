//! The command line argument parser we have inherited from the Nintendo CLI.
//!
//! This is the exact same arguments, that do the exact same things as an
//! official version of `findbridge`. Specifically v5.1.

/// The top-level command line options.
#[allow(
	// Clippy -- these are CLI flags.
	//
	// It doesn't make a ton of sense as an enum, but could work i guess, but i
	// just like this better.
	clippy::struct_excessive_bools,
)]
#[derive(Debug)]
pub struct CliOpts {
	/// Scan for all bridges present on the available network interfaces.
	pub all: bool,
	/// An argument to find a specific hostbridge.
	pub find_specific: Option<String>,
	/// Display "detailed" bridge information.
	///
	/// Some information and details are not displayed when searching by name.
	pub detail: bool,
	/// If we had an error while processing.
	pub had_error: bool,
	/// If we should display the help text.
	pub help: bool,
	/// Force `arg[0]` to be interpreted as a mac, and not a name/ip.
	pub is_forced_mac: bool,
	/// Display results in a list format rather than a table.
	///
	/// Also can be referenced as `-getinfo`.
	pub list: bool,
	/// If extra output will be produced, and displayed to the user.
	pub verbose: bool,
}
impl CliOpts {
	pub fn print_help() {
		println!(
			r#"findbridge v5.1 - Copyright (c) 2011 Nintendo Co., Ltd.

  Scans the network for the specified bridge or all bridges,
  and queries for details.

Usage:
  findbridge [options] <name> or <ip_address> or <mac_addr>

Options:
  -v..........Verbose mode.
  -h..........prints this help text.
  -all........Scan for all bridges present on the
              available network interfaces.
  -detail.....Displays detailed bridge information
              Some details are not available when searching by name.
  -list.......Display results in a list format rather than a table
  -getinfo....Same as "-list" (deprecated)
  -mac........Find a bridge and retrieve its information
              by the specified MAC address.
Arguments:
  <name>......Name of the bridge to be queried.
  <ip_addr>...IP address of the bridge to be queried.
  <mac_addr>..MAC address of the bridge to be queried."#
		);
	}
}
impl<Ty: Iterator<Item = String>> From<Ty> for CliOpts {
	fn from(arguments: Ty) -> Self {
		let mut opts = Self {
			all: false,
			detail: false,
			find_specific: None,
			had_error: false,
			help: false,
			is_forced_mac: false,
			list: false,
			verbose: false,
		};

		let mut set_any = false;
		for item in arguments {
			match item.as_str() {
				"-v" => opts.verbose = true,
				"-h" | "/?" => opts.help = true,
				"-all" => opts.all = true,
				"-detail" => opts.detail = true,
				"-list" | "-getinfo" => opts.list = true,
				"-mac" => opts.is_forced_mac = true,
				_ => {
					if opts.find_specific.is_some() {
						// This outputs TWO newlines.
						println!("ERROR: Search for only one CAT-DEV (or use \"-all\" flag)!\n");
						opts.had_error = true;
						opts.help = true;
						break;
					}

					opts.find_specific = Some(item);
				}
			}
			set_any = true;
		}
		if opts.is_forced_mac && opts.find_specific.is_none() {
			println!("ERROR: MAC address required with \"-mac\" flag!");
			opts.had_error = true;
			opts.help = true;
		}
		if !set_any {
			opts.help = true;
		}

		opts
	}
}
