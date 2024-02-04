//! The command line argument parser we have inherited from the Nintendo CLI.
//!
//! This is the exact same arguments, that do the exact same things as an
//! official version of `mionps`.

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
	/// If we should do a hex dump
	pub hex_dump: bool,
	/// The IP the user has specified to dump the parameter space.
	pub ip_address: Option<String>,
	/// The offset to the parameter to read.
	pub offset: Option<u16>,
	/// The timeout in milliseconds, if this can't be parsed as a number, will be
	/// set to 0.
	pub timeout_ms: Option<u64>,
	/// The value to set too, may be empty if not a u8.
	pub set_to_value: Option<u64>,
	/// If someone specified the '-s' flag at least once.
	pub tried_to_set_value: bool,
	/// Whether or not we should print out verbose output.
	pub verbose: bool,
	/// Yes... the official tool does act differently if verbose appeared before/after the ip.
	pub verbose_appeared_before_ip: bool,
}
impl CliOpts {
	pub fn print_help() {
		println!(
			r#"Usage: mionps [-t timeout] [-v] [-d MION_IP] | MION_IP OFFSET [-s VALUE]
where: -v        verbose
       -d        hex dump of the param space
       -t        sets timeout value in milliseconds
       MION_IP   IP address of the MION device
       OFFSET    byte offset(0-511) of the value to get/set
       -s VALUE  set the value(0-255) at the offset"#
		);
	}
}
impl<Ty: Iterator<Item = String>> From<Ty> for CliOpts {
	fn from(arguments: Ty) -> Self {
		let mut opts = Self {
			hex_dump: false,
			ip_address: None,
			offset: None,
			set_to_value: None,
			timeout_ms: None,
			tried_to_set_value: false,
			verbose: false,
			verbose_appeared_before_ip: false,
		};

		let mut next_is_set = false;
		let mut next_is_timeout = false;
		let mut read_offset_like = false;
		for item in arguments {
			if next_is_set {
				opts.set_to_value = item.parse::<u64>().ok();
				next_is_set = false;
				continue;
			}
			if next_is_timeout {
				opts.timeout_ms = Some(item.parse::<u64>().unwrap_or_default());
				next_is_timeout = false;
				continue;
			}

			match item.as_str() {
				"-d" => opts.hex_dump = true,
				"-t" => next_is_timeout = true,
				"-s" => {
					next_is_set = true;
					opts.tried_to_set_value = true;
				}
				"-v" => {
					opts.verbose = true;
					if opts.ip_address.is_none() {
						opts.verbose_appeared_before_ip = true;
					}
				}
				_ => {
					if opts.ip_address.is_none() {
						opts.ip_address = Some(item);
						continue;
					} else if !read_offset_like {
						if let Ok(value) = item.parse::<u16>() {
							opts.offset = Some(value);
						}
						read_offset_like = true;
						continue;
					}
				}
			}
		}

		opts
	}
}
