//! The command line argument parser we have inherited from the Nintendo CLI.
//!
//! This is the exact same arguments, that do the exact same things as an
//! official version of `getbridgeconfig`.

/// The top-level command line options.
#[derive(Debug)]
pub struct CliOpts {
	/// List all the bridges that this host has to offer.
	pub all: bool,
	/// If we should only list the default Bridge
	pub default: bool,
	/// If we should show the help page.
	pub help: bool,
}
impl CliOpts {
	pub fn print_help() {
		println!(
			"getbridge v1.0 - Copyright (c) 2011 Nintendo Co., Ltd.

  Returns the currently active CAT-DEV host bridge used by
  this PC, or the list of all previously used host bridges.

Usage:
  getbridge [options]

Options:
  -all.......Returns all recorded bridges used by this host.
  -default...Returns the default bridge used by this host.\n"
		);
	}
}
impl<Ty: Iterator<Item = String>> From<Ty> for CliOpts {
	fn from(arguments: Ty) -> Self {
		let mut opts = Self {
			all: false,
			default: false,
			help: false,
		};

		let mut set_any = false;
		for item in arguments {
			match item.as_str() {
				"-h" | "/?" => opts.help = true,
				"-all" => opts.all = true,
				"-default" => opts.default = true,
				_ => {
					// Ignore all other options.
				}
			}
			set_any = true;
		}
		if !set_any {
			opts.help = true;
		}

		opts
	}
}
