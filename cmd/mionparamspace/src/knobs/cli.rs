//! The command line argument parser we have inherited from the Nintendo CLI.
//!
//! This is the exact same arguments, that do the exact same things as an
//! official version of `mionparamspace`.

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
	pub dump: bool,
	pub ip_address: Option<String>,
	pub mixed_args: bool,
	pub offset: Option<u64>,
	pub out_of_order: bool,
	pub set_to_value: Option<u64>,
	pub too_many_parameters: bool,
	pub tried_to_set: bool,
	pub verbose: bool,
}
impl CliOpts {
	pub fn print_help() {
		println!(
			r#"Usage: mionparamspace [-d MION_IP] | [-v] MION_IP OFFSET [-s VALUE]
where: -v        verbose
       -d        hex dump of the param space
       MION_IP   IP address of the MION device
       OFFSET    byte offset(0-511) of the value to get/set
       -s VALUE  set the value(0-255) at the offset"#
		);
	}
}
impl<Ty: Iterator<Item = String>> From<Ty> for CliOpts {
	fn from(arguments: Ty) -> Self {
		let mut opts = Self {
			dump: false,
			ip_address: None,
			mixed_args: false,
			offset: None,
			out_of_order: false,
			set_to_value: None,
			too_many_parameters: false,
			tried_to_set: false,
			verbose: false,
		};

		let mut next_is_set = false;
		for item in arguments {
			if next_is_set {
				opts.set_to_value = item.parse::<u64>().ok();
				next_is_set = false;
			}

			match item.as_str() {
				"-d" => {
					opts.dump = true;
					if opts.offset.is_some() {
						opts.too_many_parameters = true;
					}
				}
				"-s" => {
					next_is_set = true;
					opts.tried_to_set = true;
					if opts.set_to_value.is_some() {
						opts.too_many_parameters = true;
						break;
					}
				}
				"-v" => {
					opts.verbose = true;
					if opts.tried_to_set || opts.ip_address.is_some() {
						opts.out_of_order = true;
					}
				}
				_ => {
					if opts.ip_address.is_none() {
						opts.ip_address = Some(item);
						if opts.tried_to_set {
							opts.out_of_order = true;
						}
					} else if opts.offset.is_none() {
						if opts.dump {
							opts.too_many_parameters = true;
						}
						opts.offset = item.parse::<u64>().ok();
					} else {
						opts.too_many_parameters = true;
						break;
					}
				}
			}
		}

		if (opts.tried_to_set || opts.verbose) && opts.dump {
			opts.mixed_args = true;
		}

		opts
	}
}
