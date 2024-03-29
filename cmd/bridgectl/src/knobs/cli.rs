//! Defines the command line interface a.k.a. all the arguments & flags.

use clap::Parser;
use std::{net::Ipv4Addr, path::PathBuf};

#[derive(Parser, Debug)]
#[clap(disable_help_flag = true, disable_help_subcommand = true)]
#[command(about, author, name = "bridgectl", propagate_version = true, version)]
pub struct CliArguments {
	#[arg(
		global = true,
		long = "bridge-state-path",
		alias = "bridge_state_path",
		help = "The path to the `bridge_env.ini` file to use.",
		long_help = "The path to the `bridge_env.ini` file to use if it's not in the default location."
	)]
	pub bridge_state_path: Option<PathBuf>,
	#[arg(
		global = true,
		long = "bridge-control-port-override",
		alias = "bridge_control_port_override",
		help = "A way to override the control port which should never be needed.",
		long_help = "Allow overriding the scanning port aka CONTROL port for finding cat-dev bridges."
	)]
	pub control_port_override: Option<u16>,
	#[command(subcommand)]
	pub commands: Option<Subcommands>,
	#[arg(
		global = true,
		short = 'h',
		long = "help",
		help = "Display the help page for your command rather than running it.",
		long_help = "Show the help output for either the top level cli, or a particular subcommand. This will always be prioritized."
	)]
	pub help: bool,
	#[arg(
		global = true,
		short = 'j',
		long = "json",
		help = "Ensures all logging comes out in JSON instead of text.",
		long_help = "Switch all logging and output to JSON for machine parsable output. NOTE: there is no necissarily guaranteed structure, though we will not break it unnecissarily."
	)]
	pub json: bool,
	#[arg(
		global = true,
		long = "scan-early-timeout-seconds",
		alias = "scan_early_timeout_seconds",
		help = "The amount of seconds to wait before bailing early when scanning for a bridge (by default this is 3).",
		long_help = "CAT-DEV's MUST respond to broadcasts within 10 seconds, but in reality most folks only have one cat-dev / non busy networks were they will respond faster, in this case it's generally better to exit early. How early we decide to exit is controlled by this variable."
	)]
	pub scan_timeout: Option<u64>,
}

#[derive(Parser, Debug)]
#[clap(disable_help_flag = true, disable_help_subcommand = true)]
pub enum Subcommands {
	/// Add, or update a bridge to your local configuration file so it can be used quickly later on.
	#[command(name = "add", visible_alias = "update")]
	AddOrUpdate {
		#[arg(
			short = 'n',
			long = "name",
			help = "The name of the bridge to add as a flag.",
			long_help = "The name of the bridge to add as a flag, conflicts with the positional argument."
		)]
		bridge_name: Option<String>,
		#[arg(
			short = 'i',
			long = "bridge-ip",
			alias = "bridge_ip",
			help = "The IP Address of the bridge to add as a flag.",
			long_help = "The IP Address of the bridge to add as a flag, conflicts with the positional argument."
		)]
		bridge_ipaddr: Option<Ipv4Addr>,
		#[arg(
			index = 1,
			help = "The name, or ip address of the bridge as a positional argument.",
			long_help = "The name, or ip address of the bridge to add as a positional argument. If you've not specified `--name` this will be the bridge name, if you have specified `--name`, but have not specified `--bridge-ip` we will attempt to use this as an ip."
		)]
		bridge_name_positional: Option<String>,
		#[arg(
			index = 2,
			help = "The IP Address of the bridge to add.",
			long_help = "The IP Address of the bridge to add as a positional argument, conflicts with the flag version of `--bridge-ip`."
		)]
		bridge_ip_positional: Option<Ipv4Addr>,
		#[arg(
			long = "default",
			help = "Makes this bridge the default.",
			long_help = "Sets the bridge as the default bridge to use when opening new shells, with this you don't need to separately call `set-default`."
		)]
		set_default: bool,
	},
	/// Attempt to power on a MION, so you can actually use it.
	#[command(
		name = "boot",
		visible_aliases = ["power-on", "power_on"],
	)]
	Boot {
		#[arg(
			short = 'd',
			long = "default",
			help = "Set the parameters on the default bridge.",
			long_help = "A shortcut to set parameters on the default bridge, not needing to specify any other lookup fields."
		)]
		default: bool,
		#[arg(
			short = 'i',
			long = "ip",
			help = "The IP of the bridge to set the parameters on.",
			long_help = "Set the parameters of the bridge located at this IP address."
		)]
		bridge_ipaddr: Option<Ipv4Addr>,
		#[arg(
			short = 'm',
			long = "mac-address",
			alias = "mac_address",
			help = "The Mac Address of the bridge to set the parameters on.",
			long_help = "Set the parameters of the bridge found by searching for the bridge with this MAC Address."
		)]
		bridge_mac: Option<String>,
		#[arg(
			short = 'n',
			long = "name",
			help = "The Name of the bridge to set the parameters on.",
			long_help = "Set the parameters of the bridge found by searching for the bridge with this Name."
		)]
		bridge_name: Option<String>,
		#[arg(
			index = 1,
			help = "Search for a bridge with a particular name/ip/mac address.",
			long_help = "If you don't want to specify what bridge you want to set parameters on with `--ip`, `--mac-address`, or `--name` you can just pass in a positional argument where we can guess how to find the bridge."
		)]
		bridge_name_positional: Option<String>,
		#[arg(
			long = "boot-without-pcfs",
			alias = "boot_without_pcfs",
			help = "Just boot the device without PCFS",
			long_help = "Disable almost all other options, and just boot the device without any connection to the PC."
		)]
		without_pcfs: bool,
		#[arg(
			short = 's',
			long = "serial-port-path",
			alias = "serial_port_path",
			help = "The path to the serial port to use (conflicts with the positional argument).",
			long_help = "The path to the serial port to use, on Windows you should use something like 'COM1', 'COM2', etc., on Linux this should be the full path to the device (conflicts with the positional argument)."
		)]
		serial_port_flag: Option<PathBuf>,
		#[arg(
			index = 2,
			help = "The path to the serial port to use (conflicts with the flag).",
			long_help = "The path to the serial port to use, on Windows you should use something like 'COM1', 'COM2', etc., on Linux this should be the full path to the device (conflicts with the flag)."
		)]
		serial_port_positional: Option<PathBuf>,
	},
	/// Dump the entire parameter space of a MION.
	#[command(
		name = "dump-parameters",
		visible_aliases = ["dp", "dump_parameters"],
	)]
	DumpParameters {
		#[arg(
			short = 'd',
			long = "default",
			help = "Get the parameters from the default bridge.",
			long_help = "A shortcut to get parameters from the default bridge, not needing to specify any other lookup fields."
		)]
		default: bool,
		#[arg(
			short = 'i',
			long = "ip",
			help = "The IP Address of the bridge to get parameters from.",
			long_help = "Get the parameters of the bridge located at this IP address."
		)]
		bridge_ipaddr: Option<Ipv4Addr>,
		#[arg(
			short = 'm',
			long = "mac-address",
			alias = "mac_address",
			help = "The Mac Address of the bridge to get parameters from.",
			long_help = "Get the parameters of the bridge found by searching for the bridge with this MAC Address."
		)]
		bridge_mac: Option<String>,
		#[arg(
			short = 'n',
			long = "name",
			help = "The Name of the bridge to get parameters from.",
			long_help = "Get the parameters of the bridge found by searching for the bridge with this Name."
		)]
		bridge_name: Option<String>,
		#[arg(
			index = 1,
			help = "Search for a bridge with a particular name/ip/mac address.",
			long_help = "If you don't want to specify what bridge you want to get parameters from with `--ip`, `--mac-address`, or `--name` you can just pass in a positional argument where we can guess how to find the bridge."
		)]
		bridge_name_positional: Option<String>,
		#[arg(
			short = 'p',
			long = "port",
			help = "The 'parameter space' port to use.",
			long_help = "The 'parameter space' port to use. Official tools don't support changing this, but it is configurable in `setup.cgi`."
		)]
		parameter_space_port: Option<u16>,
	},
	/// Get info on a single bridge, using any piece of information we can search for.
	#[command(name = "get")]
	Get {
		#[arg(
			short = 'd',
			long = "default",
			help = "Just fetch the default bridge.",
			long_help = "A shortcut to fetch the default bridge, cannot specify any other filters."
		)]
		default: bool,
		#[arg(
			short = 'i',
			long = "ip",
			help = "Search for a bridge with a particular IP Address.",
			long_help = "Search for a bridge that's located at a particular IPv4 address (can also be specified as a positional argument)."
		)]
		bridge_ipaddr: Option<Ipv4Addr>,
		#[arg(
			short = 'm',
			long = "mac-address",
			alias = "mac_address",
			help = "Search for a bridge with a particular MAC Address.",
			long_help = "Search for a bridge with a particular MAC Address, this will return the first bridge with this MAC as it should be unique, can also be specified as a positional argument."
		)]
		bridge_mac: Option<String>,
		#[arg(
			short = 'n',
			long = "name",
			help = "Search for a bridge with a particular name.",
			long_help = "Search for a bridge with a particular name, this will return the first bridge with this name as it should be unique. It can be specified as a positional argument though if it looks like a MAC Address, or IP it may be filtered that way (you may want to use `--bridge-name` for name always)."
		)]
		bridge_name: Option<String>,
		#[arg(
			index = 1,
			help = "Search for a bridge with a particular name/ip/mac address.",
			long_help = "If you don't want to specify what type you're searching for with `--ip`, `--mac-address`, or `--name` you can just pass in a positional argument where we can guess"
		)]
		bridge_name_positional: Option<String>,
		#[arg(
			short = 't',
			long = "table-output",
			alias = "table_output",
			help = "Output the list of bridges as a particular table.",
			long_help = "Rather than outputting the information as a bunch of log lines, output the information in a table"
		)]
		output_as_table: bool,
	},
	/// Get the parameters from the parameter space of a MION.
	#[command(
		name = "get-parameters",
		visible_aliases = ["gp", "get_parameters"],
	)]
	GetParameters {
		#[arg(
			short = 'd',
			long = "default",
			help = "Get the parameters from the default bridge.",
			long_help = "A shortcut to get parameters from the default bridge, not needing to specify any other lookup fields."
		)]
		default: bool,
		#[arg(
			short = 'i',
			long = "ip",
			help = "The IP Address of the bridge to get parameters from.",
			long_help = "Get the parameters of the bridge located at this IP address."
		)]
		bridge_ipaddr: Option<Ipv4Addr>,
		#[arg(
			short = 'm',
			long = "mac-address",
			alias = "mac_address",
			help = "The Mac Address of the bridge to get parameters from.",
			long_help = "Get the parameters of the bridge found by searching for the bridge with this MAC Address."
		)]
		bridge_mac: Option<String>,
		#[arg(
			short = 'n',
			long = "name",
			help = "The Name of the bridge to get parameters from.",
			long_help = "Get the parameters of the bridge found by searching for the bridge with this Name."
		)]
		bridge_name: Option<String>,
		#[arg(
			index = 1,
			help = "Search for a bridge with a particular name/ip/mac address.",
			long_help = "If you don't want to specify what bridge you want to get parameters from with `--ip`, `--mac-address`, or `--name` you can just pass in a positional argument where we can guess how to find the bridge."
		)]
		bridge_name_positional: Option<String>,
		#[arg(
			index = 2,
			help = "The list of bridge parameters to fetch by name or index (separated by comma).",
			long_help = "The list of parameters you want to fetch separated by comma, this can be the name of the field, or the index of the field."
		)]
		parameter_names_positional: Option<String>,
		#[arg(
			short = 'p',
			long = "port",
			help = "The 'parameter space' port to use.",
			long_help = "The 'parameter space' port to use. Official tools don't support changing this, but it is configurable in `setup.cgi`."
		)]
		parameter_space_port: Option<u16>,
	},
	/// An alternative to `-h`, or `--help` to show the help for the top level CLI.
	#[command(name = "help")]
	Help {},
	/// List all the bridges on your network or all the bridges you've connected to in the past.
	#[command(name = "list", visible_alias = "ls")]
	List {
		#[arg(
			short = 'c',
			long = "cached",
			help = "Only list the bridges you know about locally.",
			long_help = "Don't scan the network for all the bridges actively around, and list only the bridges you know about locally."
		)]
		use_cache: bool,
		#[arg(
			short = 't',
			long = "table-output",
			alias = "table_output",
			help = "Output the list of bridges as a particular table.",
			long_help = "Rather than outputting the information as a bunch of log lines, output the information in a table"
		)]
		output_as_table: bool,
	},
	/// List the available serial ports, to try and find the devices you can use
	/// to tail logs from.
	#[command(
		name = "list-serial-ports",
		visible_aliases = ["ls-serial-ports", "lssp", "list_serial_ports", "ls_serial_ports"],
	)]
	ListSerialPorts {},
	/// Remove a bridge from your local configuration file.
	#[command(name = "remove", visible_alias = "rm")]
	Remove {
		#[arg(
			short = 'n',
			long = "name",
			help = "The bridge name to remove.",
			long_help = "The bridge name to remove, you can also specify this as a positional argument, but you cannot specify both."
		)]
		bridge_name: Option<String>,
		#[arg(
			index = 1,
			help = "The bridge name to remove as a positional argument as opposed to a flag.",
			long_help = "If you don't want to specify what device you're wanting to remove with `--name` you can just pass in a positional argument."
		)]
		bridge_name_positional: Option<String>,
	},
	/// Used to change the default bridge we load up automatically.
	#[command(name = "set-default", visible_alias = "set_default")]
	SetDefault {
		#[arg(
			short = 'n',
			long = "name",
			help = "The bridge name to remove.",
			long_help = "The bridge name to remove, you can also specify this as a positional argument, but you cannot specify both."
		)]
		bridge_name: Option<String>,
		#[arg(
			index = 1,
			help = "The bridge name to remove as a positional argument as opposed to a flag.",
			long_help = "If you don't want to specify what device you're wanting to remove with `--name` you can just pass in a positional argument."
		)]
		bridge_name_positional: Option<String>,
	},
	/// Set bytes in the parameter space of a MION.
	#[command(
		name = "set-parameters",
		visible_aliases = ["sp", "set_parameters"],
	)]
	SetParameters {
		#[arg(
			short = 'd',
			long = "default",
			help = "Set the parameters on the default bridge.",
			long_help = "A shortcut to set parameters on the default bridge, not needing to specify any other lookup fields."
		)]
		default: bool,
		#[arg(
			short = 'i',
			long = "ip",
			help = "The IP of the bridge to set the parameters on.",
			long_help = "Set the parameters of the bridge located at this IP address."
		)]
		bridge_ipaddr: Option<Ipv4Addr>,
		#[arg(
			short = 'm',
			long = "mac-address",
			alias = "mac_address",
			help = "The Mac Address of the bridge to set the parameters on.",
			long_help = "Set the parameters of the bridge found by searching for the bridge with this MAC Address."
		)]
		bridge_mac: Option<String>,
		#[arg(
			short = 'n',
			long = "name",
			help = "The Name of the bridge to set the parameters on.",
			long_help = "Set the parameters of the bridge found by searching for the bridge with this Name."
		)]
		bridge_name: Option<String>,
		#[arg(
			index = 1,
			help = "Search for a bridge with a particular name/ip/mac address.",
			long_help = "If you don't want to specify what bridge you want to set parameters on with `--ip`, `--mac-address`, or `--name` you can just pass in a positional argument where we can guess how to find the bridge."
		)]
		bridge_name_positional: Option<String>,
		#[arg(
			index = 2,
			help = "The list of bridge parameters to set in the form of `(name or index)=(value)`.",
			long_help = "The list of bridge parameters to set in the form of `(name or index)=(value)`. You can specify multiple parameters to set by using ',',"
		)]
		parameter_names_positional: Option<String>,
		#[arg(
			short = 'p',
			long = "port",
			help = "The 'parameter space' port to use.",
			long_help = "The 'parameter space' port to use. Official tools don't support changing this, but it is configurable in `setup.cgi`."
		)]
		parameter_space_port: Option<u16>,
	},
	/// Tail the logs of a serial port.
	#[command(
		name = "tail",
		visible_aliases = ["tail-serial-port", "tail_serial_port"],
	)]
	Tail {
		#[arg(
			short = 's',
			long = "serial-port-path",
			alias = "serial_port_path",
			help = "The path to the serial port to use (conflicts with the positional argument).",
			long_help = "The path to the serial port to use, on Windows you should use something like 'COM1', 'COM2', etc., on Linux this should be the full path to the device (conflicts with the positional argument)."
		)]
		serial_port_flag: Option<PathBuf>,
		#[arg(
			index = 1,
			help = "The path to the serial port to use (conflicts with the flag).",
			long_help = "The path to the serial port to use, on Windows you should use something like 'COM1', 'COM2', etc., on Linux this should be the full path to the device (conflicts with the flag)."
		)]
		serial_port_positional: Option<PathBuf>,
	},
}
impl Subcommands {
	/// If this subcommand matches a particular name.
	#[allow(unused)]
	#[must_use]
	pub fn name_matches(&self, name: &str) -> bool {
		match self {
			Self::AddOrUpdate {
				bridge_name,
				bridge_ipaddr,
				bridge_name_positional,
				bridge_ip_positional,
				set_default,
			} => name == "add" || name == "update",
			Self::Boot {
				default,
				bridge_ipaddr,
				bridge_mac,
				bridge_name,
				bridge_name_positional,
				without_pcfs,
				serial_port_flag,
				serial_port_positional,
			} => name == "boot" || name == "power-on" || name == "power_on",
			Self::DumpParameters {
				default,
				bridge_ipaddr,
				bridge_mac,
				bridge_name,
				bridge_name_positional,
				parameter_space_port,
			} => name == "dump-parameters" || name == "dump_parameters" || name == "dp",
			Self::Get {
				default,
				bridge_ipaddr,
				bridge_mac,
				bridge_name,
				bridge_name_positional,
				output_as_table,
			} => name == "get",
			Self::GetParameters {
				default,
				bridge_ipaddr,
				bridge_mac,
				bridge_name,
				bridge_name_positional,
				parameter_names_positional,
				parameter_space_port,
			} => name == "get-parameters" || name == "get_parameters" || name == "gp",
			Self::Help {} => name == "help",
			Self::List {
				use_cache,
				output_as_table,
			} => name == "list" || name == "ls",
			Self::ListSerialPorts {} => {
				name == "list-serial-ports"
					|| name == "ls-serial-ports"
					|| name == "list_serial_ports"
					|| name == "ls_serial_ports"
					|| name == "lssp"
			}
			Self::Remove {
				bridge_name,
				bridge_name_positional,
			} => name == "remove" || name == "rm",
			Self::SetDefault {
				bridge_name,
				bridge_name_positional,
			} => name == "set-default" || name == "set_default",
			Self::SetParameters {
				default,
				bridge_ipaddr,
				bridge_mac,
				bridge_name,
				bridge_name_positional,
				parameter_names_positional,
				parameter_space_port,
			} => name == "set-parameters" || name == "set_parameters" || name == "sp",
			Self::Tail {
				serial_port_flag,
				serial_port_positional,
			} => name == "tail" || name == "tail-serial-port" || name == "tail_serial_port",
		}
	}
}
