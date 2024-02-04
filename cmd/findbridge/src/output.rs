//! All the functions we use for manually running a bunch of `println!`
//! statements to show to the user.
//!
//! This has a whole bunch of hacky `print!` vs `println!` on different flags
//! that may not always be clear to you as a user. I promise this is the exact
//! same way the original tools acted, and the output does match EXACTLY.

use cat_dev::mion::proto::control::MionIdentity;
use network_interface::Addr;
use std::io::Write;

/// Print the header to show before we actually end up printing all of the
/// bridges.
pub fn print_bridge_header(is_detailed: bool, is_list: bool) {
	if is_detailed {
		println!("\nNote: Bridge Firmware older than 0.0.14.63 will not return some fields.\n");
		println!(
			"Bridge Name                    IP Address         FW Rev     FPGA        Mac Addr       SDK    Boot   Cafe"
		);
		println!(
			"                                                              Rev                       Rev    Mode  On/Off"
		);
		if is_list {
			println!(
				"----------------------------------------------------------------------------------------------------"
			);
		} else {
			print!(
				"----------------------------------------------------------------------------------------------------"
			);
			_ = std::io::stdout().flush();
		}
	} else {
		println!("\nBridge Name                    : IP Address");
		println!("-----------------------------------------------------");
	}
}

/// Print out an actual bridge we've discovered.
pub fn print_bridge(identity: &MionIdentity, is_detailed: bool, is_list: bool, is_first: bool) {
	if is_list {
		print_identity_list_view(identity, is_detailed);
	} else {
		print_identity_column_view(identity, is_detailed, is_first);
	}
}

/// Create a hook to log when we start scanning a particular network address.
pub fn create_interface_logging_hook(verbose: bool) -> impl Fn(&'_ Addr) + Clone + Send + 'static {
	if verbose {
		|addr: &Addr| {
			println!("Scanning for bridges on interface {}...", addr.ip());
		}
	} else {
		|_addr: &Addr| {}
	}
}

/// Print the "helpful" suggestions findbridge can recommend to a user.
pub fn print_verbose_search_suggestions() {
	println!(
		"  - Ensure that broadcast packets are allowed on the network.
  - Ensure that DHCP is enabled on the network.
  - Ensure that the host bridge is connected to the same network
    and has been powered on."
	);
}

fn print_identity_list_view(identity: &MionIdentity, is_detailed: bool) {
	if is_detailed {
		println!(
			"
Bridge name        : '{}'
IP address         : {}
MAC address        : {}
FPGA image version : {}
Firmware version   : {}
SDK version        : {}
Boot Mode          : {}
Power Status       : {}",
			identity.name(),
			identity.ip_address(),
			identity.mac_address(),
			identity.fpga_version(),
			identity.firmware_version(),
			identity
				.detailed_sdk_version()
				.unwrap_or("Unknown".to_owned()),
			identity
				.detailed_boot_type()
				.map_or("Unknown".to_owned(), |val| format!("{val}")),
			identity
				.detailed_is_cafe_on()
				.map_or("Unknown", |status| if status { "ON" } else { "OFF" }),
		);
	} else {
		println!(
			"
Bridge name        : '{}'
IP address         : {}
MAC address        : {}
FPGA image version : {}
Firmware version   : {}",
			identity.name(),
			identity.ip_address(),
			identity.mac_address(),
			identity.fpga_version(),
			identity.firmware_version(),
		);
	}
}

fn print_identity_column_view(identity: &MionIdentity, is_detailed: bool, is_first: bool) {
	if is_detailed {
		println!();
		let default_str = get_detailed_view_minus_extra_fields(identity);
		let first_prefix = if is_first { "\n" } else { "" };

		let Some(mut sdk_version) = identity.detailed_sdk_version() else {
			print!("{first_prefix}{default_str}");
			return;
		};
		if sdk_version.find('.').unwrap_or(0) != 2 && !sdk_version.is_empty() {
			sdk_version = format!(" {sdk_version}");
		}

		let Some(boot_type) = identity.detailed_boot_type() else {
			print!("{first_prefix}{default_str}");
			return;
		};
		let Some(is_cafe_on) = identity.detailed_is_cafe_on() else {
			print!("{first_prefix}{default_str}");
			return;
		};

		print!(
			"{first_prefix}{default_str} {sdk_version} {boot_type}  {}",
			if is_cafe_on { "ON" } else { "OFF" }
		);
		_ = std::io::stdout().flush();
	} else {
		let mut name = String::from(identity.name());
		if name.len() > 31 {
			name = name[..28].to_owned();
			name += "...";
		} else {
			while name.len() < 31 {
				name.push(' ');
			}
		}
		println!("{name}: {}", identity.ip_address());
	}
}

fn get_detailed_view_minus_extra_fields(identity: &MionIdentity) -> String {
	let mut name = String::from(identity.name());
	if name.len() > 32 {
		name = name[..29].to_owned();
		name += "...";
	} else {
		while name.len() < 32 {
			name.push(' ');
		}
	}

	let mut ip = format!("{}", identity.ip_address());
	while ip.len() < 16 {
		ip.push(' ');
	}
	let mut version = identity.firmware_version();
	while version.len() < 11 {
		version.push(' ');
	}
	let mut fpga_version = identity.detailed_fpga_version();
	while fpga_version.len() < 10 {
		fpga_version.push(' ');
	}
	let mac = format!("{}", identity.mac_address());

	format!("{name}{ip}{version}{fpga_version}{mac}")
}
