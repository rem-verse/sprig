//! The command line argument parser we have inherited from the Nintendo CLI.
//!
//! This is the exact same arguments, that do the exact same things as an
//! official version of `setbridgeconfig`.

/// The top-level command line options.
#[derive(Debug)]
pub struct CliOpts {
	/// The amount of arguments that were specified on the command line.
	pub arg_count: usize,
	/// The IP Address of the bridge that we should add/set as default.
	pub bridge_ipaddr: Option<String>,
	/// The name of the bridge that we should add/set as default/delete.
	pub bridge_name: Option<String>,
	/// If we should set this bridge as the default bridge.
	pub default: bool,
	/// If we should delete this bridge.
	pub delete: bool,
	/// If we should protect from overwriting a bridge or being deleted.
	pub protect: bool,
}
impl CliOpts {
	pub fn print_help() {
		println!(
			"setbridge v4.1 - Copyright (c) 2011-2013 Nintendo Co., Ltd.

  Facilitates access to one or more CAT-DEVs via network interface.

  Specifying the name and IP address of a bridge will add the bridge
  to the internal history (which is maintained within an INI file).

  Using the '-default' option sets the bridge specified by <name> as
  the default target for all CAT-DEV SDK operations (such as caferun).

  The default CAT-DEV bridge is maintained in an environment variable.
  This allows separate Cafe shells to operate different CAT-DEVs
  simultaneously.

Usage:
  setbridgeconfig [options] <name> [<ip_addr>]

Options:
  -default......Sets as default the bridge specified by <name>.
                If <name> and <ip_addr> are specified, adds an
                entry to the INI file and sets it as the default.
  -protect......Prevents aleady existing entries from being over-
                written by <name> and <ip_addr>.
  -d............deletes an existing entry\n"
		);
	}
}
impl<Ty: Iterator<Item = String>> From<Ty> for CliOpts {
	fn from(arguments: Ty) -> Self {
		let mut opts = Self {
			arg_count: 1,
			bridge_ipaddr: None,
			bridge_name: None,
			default: false,
			delete: false,
			protect: false,
		};

		for item in arguments {
			opts.arg_count += 1;
			match item.as_str() {
				"-default" => opts.default = true,
				"-protect" => opts.protect = true,
				"-d" => opts.delete = true,
				_ => {
					if !item.starts_with('-') {
						if opts.bridge_name.is_none() {
							opts.bridge_name = Some(item);
						} else if opts.bridge_ipaddr.is_none() {
							opts.bridge_ipaddr = Some(item);
						}
					}
				}
			}
		}

		opts
	}
}
