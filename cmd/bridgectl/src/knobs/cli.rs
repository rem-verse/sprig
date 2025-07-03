//! Defines the command line interface a.k.a. all the arguments & flags.

use clap::{Args, Parser, Subcommand};
use mac_address::MacAddress;
use std::{
	fmt::{Display, Formatter, Result as FmtResult},
	net::Ipv4Addr,
	path::PathBuf,
};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

#[derive(Parser, Debug)]
#[clap(disable_help_flag = true, disable_help_subcommand = true)]
#[command(about, author, name = "bridgectl", propagate_version = true, version)]
pub struct CliArguments {
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
}

#[derive(Parser, Debug)]
#[clap(disable_help_flag = true, disable_help_subcommand = true)]
pub enum Subcommands {
	/// Add, or update a bridge to your local configuration file so it can be used quickly later on.
	#[command(name = "add", visible_alias = "update")]
	AddOrUpdate {
		// ///////////////////////////////////////////////////
		// Shared Flags for targeting a single bridge.
		// ///////////////////////////////////////////////////
		#[command(flatten)]
		scan_flags: BridgeScanFlags,
		#[command(flatten)]
		target_flags: TargetBridgeFlags,
		#[command(flatten)]
		bridge_config_flags: BridgeConfigurationFlags,
		#[arg(
			index = 1,
			help = "Search for a bridge with a particular name/ip/mac address.",
			long_help = "If you don't want to specify what bridge you want to set parameters on with `--ip`, `--mac-address`, or `--name` you can just pass in a positional argument where we can guess how to find the bridge."
		)]
		bridge_name_positional: Option<String>,
		// ///////////////////////////////////////////////////
		// Add only command arguments.
		// ///////////////////////////////////////////////////
		#[arg(
			long = "set-default",
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
		// ///////////////////////////////////////////////////
		// Shared Flags for targeting a single bridge.
		// ///////////////////////////////////////////////////
		#[command(flatten)]
		bridge_config_flags: BridgeConfigurationFlags,
		#[command(flatten)]
		scan_flags: BridgeScanFlags,
		#[command(flatten)]
		target_flags: TargetBridgeFlags,
		#[arg(
			index = 1,
			help = "Search for a bridge with a particular name/ip/mac address.",
			long_help = "If you don't want to specify what bridge you want to set parameters on with `--ip`, `--mac-address`, or `--name` you can just pass in a positional argument where we can guess how to find the bridge."
		)]
		bridge_name_positional: Option<String>,
		// ///////////////////////////////////////////////////
		// FS Emulation shared configuration flags..
		// ///////////////////////////////////////////////////
		#[command(flatten)]
		fsemul_flags: FSEmulConfigurationFlags,
		#[command(flatten)]
		shared_server_flags: SharedServerFlags,
		// ///////////////////////////////////////////////////
		// Shared Flags for targeting a single Serial Port.
		// ///////////////////////////////////////////////////
		#[arg(
			index = 2,
			help = "The path to the serial port to use (conflicts with the flag).",
			long_help = "The path to the serial port to use, on Windows you should use something like 'COM1', 'COM2', etc., on Linux this should be the full path to the device (conflicts with the flag)."
		)]
		serial_port_positional: Option<PathBuf>,
		#[command(flatten)]
		shared_serial_port_flags: SharedSerialPortFlags,
		// ///////////////////////////////////////////////////
		// Boot only command flags.
		// ///////////////////////////////////////////////////
		#[arg(
			long = "disable-pcfs-over-sata",
			alias = "disable_pcfs_over_sata",
			help = "Do not serve the device using the 'SATA' port, and use SDIO.",
			long_help = "Disable the 'SATA' server, and use the SDIO server (a significantly less performant server)."
		)]
		disable_sata: bool,
		#[arg(
			long = "parameter-space-port",
			help = "The 'parameter space' port to use.",
			long_help = "The 'parameter space' port to use. Official tools don't support changing this, but it is configurable in `setup.cgi`."
		)]
		parameter_space_port: Option<u16>,
		#[arg(
			long = "take-ownership",
			help = "If we should take over managing a MION from another host.",
			long_help = "This will allow us to start managing a MION 'stealing' control from another host. Without this the boot command will exit with an error if it's currently being managed by another host."
		)]
		take_ownership: bool,
		#[arg(
			long = "boot-without-pcfs",
			alias = "boot_without_pcfs",
			help = "Just boot the device without PCFS",
			long_help = "Disable almost all other options, and just boot the device without any connection to the PC."
		)]
		without_pcfs: bool,
	},
	/// Dump the entire parameter space of a MION.
	#[command(
		name = "dump-parameters",
		visible_aliases = ["dp", "dump_parameters"],
	)]
	DumpParameters {
		// ///////////////////////////////////////////////////
		// Shared Flags for targeting a single bridge.
		// ///////////////////////////////////////////////////
		#[command(flatten)]
		bridge_config_flags: BridgeConfigurationFlags,
		#[command(flatten)]
		scan_flags: BridgeScanFlags,
		#[command(flatten)]
		target_flags: TargetBridgeFlags,
		#[arg(
			index = 1,
			help = "Search for a bridge with a particular name/ip/mac address.",
			long_help = "If you don't want to specify what bridge you want to get parameters from with `--ip`, `--mac-address`, or `--name` you can just pass in a positional argument where we can guess how to find the bridge."
		)]
		bridge_name_positional: Option<String>,
		// ///////////////////////////////////////////////////
		// Shared flags for configuring the parameter space.
		// ///////////////////////////////////////////////////
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
		// ///////////////////////////////////////////////////
		// Shared Flags for targeting a single bridge.
		// ///////////////////////////////////////////////////
		#[command(flatten)]
		bridge_config_flags: BridgeConfigurationFlags,
		#[command(flatten)]
		scan_flags: BridgeScanFlags,
		#[command(flatten)]
		target_flags: TargetBridgeFlags,
		#[arg(
			index = 1,
			help = "Search for a bridge with a particular name/ip/mac address.",
			long_help = "If you don't want to specify what type you're searching for with `--ip`, `--mac-address`, or `--name` you can just pass in a positional argument where we can guess"
		)]
		bridge_name_positional: Option<String>,
		// ///////////////////////////////////////////////////
		// Get only command flags.
		// ///////////////////////////////////////////////////
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
		// ///////////////////////////////////////////////////
		// Shared Flags for targeting a single bridge.
		// ///////////////////////////////////////////////////
		#[command(flatten)]
		bridge_config_flags: BridgeConfigurationFlags,
		#[command(flatten)]
		scan_flags: BridgeScanFlags,
		#[command(flatten)]
		target_flags: TargetBridgeFlags,
		#[arg(
			index = 1,
			help = "Search for a bridge with a particular name/ip/mac address.",
			long_help = "If you don't want to specify what bridge you want to get parameters from with `--ip`, `--mac-address`, or `--name` you can just pass in a positional argument where we can guess how to find the bridge."
		)]
		bridge_name_positional: Option<String>,
		// ///////////////////////////////////////////////////
		// Shared flags for configuring the parameter space.
		// ///////////////////////////////////////////////////
		#[arg(
			short = 'p',
			long = "port",
			help = "The 'parameter space' port to use.",
			long_help = "The 'parameter space' port to use. Official tools don't support changing this, but it is configurable in `setup.cgi`."
		)]
		parameter_space_port: Option<u16>,
		// ///////////////////////////////////////////////////
		// Get Parameters only flags.
		// ///////////////////////////////////////////////////
		#[arg(
			index = 2,
			help = "The list of bridge parameters to fetch by name or index (separated by comma).",
			long_help = "The list of parameters you want to fetch separated by comma, this can be the name of the field, or the index of the field."
		)]
		parameter_names_positional: Option<String>,
	},
	/// An alternative to `-h`, or `--help` to show the help for the top level CLI.
	#[command(name = "help")]
	Help {},
	/// List all the bridges on your network or all the bridges you've connected to in the past.
	#[command(name = "list", visible_alias = "ls")]
	List {
		// ///////////////////////////////////////////////////
		// Shared Flags for scanning for a bridge.
		// ///////////////////////////////////////////////////
		#[command(flatten)]
		bridge_config_flags: BridgeConfigurationFlags,
		#[command(flatten)]
		scan_flags: BridgeScanFlags,
		// ///////////////////////////////////////////////////
		// List only flags.
		// ///////////////////////////////////////////////////
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
	/// Subcommands for interacting directly with custom APIs for the small board
	/// controlling all disc access (aka the MION).
	Mion {
		#[clap(subcommand)]
		subcommand: Option<MionSubcommands>,
	},
	/// Remove a bridge from your local configuration file.
	#[command(name = "remove", visible_alias = "rm")]
	Remove {
		// ///////////////////////////////////////////////////
		// Shared Flags for targeting a single bridge.
		// ///////////////////////////////////////////////////
		#[command(flatten)]
		bridge_config_flags: BridgeConfigurationFlags,
		#[command(flatten)]
		scan_flags: BridgeScanFlags,
		#[command(flatten)]
		target_flags: TargetBridgeFlags,
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
		// ///////////////////////////////////////////////////
		// Shared Flags for targeting a single bridge.
		// ///////////////////////////////////////////////////
		#[command(flatten)]
		bridge_config_flags: BridgeConfigurationFlags,
		#[command(flatten)]
		scan_flags: BridgeScanFlags,
		#[command(flatten)]
		target_flags: TargetBridgeFlags,
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
		// ///////////////////////////////////////////////////
		// Shared Flags for targeting a single bridge.
		// ///////////////////////////////////////////////////
		#[command(flatten)]
		bridge_config_flags: BridgeConfigurationFlags,
		#[command(flatten)]
		scan_flags: BridgeScanFlags,
		#[command(flatten)]
		target_flags: TargetBridgeFlags,
		#[arg(
			index = 1,
			help = "Search for a bridge with a particular name/ip/mac address.",
			long_help = "If you don't want to specify what bridge you want to set parameters on with `--ip`, `--mac-address`, or `--name` you can just pass in a positional argument where we can guess how to find the bridge."
		)]
		bridge_name_positional: Option<String>,
		// ///////////////////////////////////////////////////
		// Shared flags for configuring the parameter space.
		// ///////////////////////////////////////////////////
		#[arg(
			short = 'p',
			long = "port",
			help = "The 'parameter space' port to use.",
			long_help = "The 'parameter space' port to use. Official tools don't support changing this, but it is configurable in `setup.cgi`."
		)]
		parameter_space_port: Option<u16>,
		// ///////////////////////////////////////////////////
		// Set parameter only flags.
		// ///////////////////////////////////////////////////
		#[arg(
			index = 2,
			help = "The list of bridge parameters to set in the form of `(name or index)=(value)`.",
			long_help = "The list of bridge parameters to set in the form of `(name or index)=(value)`. You can specify multiple parameters to set by using ',',"
		)]
		parameter_names_positional: Option<String>,
	},
	/// Tail the logs of a serial port.
	#[command(
		name = "tail",
		visible_aliases = ["tail-serial-port", "tail_serial_port"],
	)]
	Tail {
		// ///////////////////////////////////////////////////
		// FS Emulation shared configuration flags..
		// ///////////////////////////////////////////////////
		#[command(flatten)]
		fsemul_flags: FSEmulConfigurationFlags,
		// ///////////////////////////////////////////////////
		// Shared Flags for targeting a single bridge.
		// ///////////////////////////////////////////////////
		#[command(flatten)]
		bridge_config_flags: BridgeConfigurationFlags,
		#[command(flatten)]
		scan_flags: BridgeScanFlags,
		#[command(flatten)]
		target_flags: TargetBridgeFlags,
		#[arg(
			index = 1,
			help = "Either a bridge name, or the path to the serial port to tail.",
			long_help = "This can be the path to the serial port, OR this can be interpreted as a bridge search parameter. If you don't want to specify what bridge you want to get parameters from with `--ip`, `--mac-address`, or `--name` you can just pass in a positional argument where we can guess how to find the bridge."
		)]
		bridge_name_or_serial_port_path: Option<String>,
		// ///////////////////////////////////////////////////
		// Shared Flags for targeting a single Serial Port.
		// ///////////////////////////////////////////////////
		#[command(flatten)]
		shared_serial_port_flags: SharedSerialPortFlags,
	},
}
impl Subcommands {
	/// If this subcommand matches a particular name.
	#[allow(unused)]
	#[must_use]
	pub fn name_matches(&self, name: &str) -> bool {
		match self {
			Self::AddOrUpdate {
				bridge_config_flags,
				scan_flags,
				target_flags,
				bridge_name_positional,
				set_default,
			} => name == "add" || name == "update",
			Self::Boot {
				bridge_config_flags,
				scan_flags,
				target_flags,
				bridge_name_positional,
				fsemul_flags,
				shared_server_flags,
				serial_port_positional,
				shared_serial_port_flags,
				disable_sata,
				parameter_space_port,
				take_ownership,
				without_pcfs,
			} => name == "boot" || name == "power-on" || name == "power_on",
			Self::DumpParameters {
				bridge_config_flags,
				scan_flags,
				target_flags,
				bridge_name_positional,
				parameter_space_port,
			} => name == "dump-parameters" || name == "dump_parameters" || name == "dp",
			Self::Get {
				bridge_config_flags,
				scan_flags,
				target_flags,
				bridge_name_positional,
				output_as_table,
			} => name == "get",
			Self::GetParameters {
				bridge_config_flags,
				scan_flags,
				target_flags,
				bridge_name_positional,
				parameter_space_port,
				parameter_names_positional,
			} => name == "get-parameters" || name == "get_parameters" || name == "gp",
			Self::Help {} => name == "help",
			Self::List {
				bridge_config_flags,
				scan_flags,
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
			Self::Mion { subcommand } => name == "mion",
			Self::Remove {
				bridge_config_flags,
				scan_flags,
				target_flags,
				bridge_name_positional,
			} => name == "remove" || name == "rm",
			Self::SetDefault {
				bridge_config_flags,
				scan_flags,
				target_flags,
				bridge_name_positional,
			} => name == "set-default" || name == "set_default",
			Self::SetParameters {
				bridge_config_flags,
				scan_flags,
				target_flags,
				bridge_name_positional,
				parameter_space_port,
				parameter_names_positional,
			} => name == "set-parameters" || name == "set_parameters" || name == "sp",
			Self::Tail {
				fsemul_flags,
				bridge_config_flags,
				scan_flags,
				target_flags,
				bridge_name_or_serial_port_path,
				shared_serial_port_flags,
			} => name == "tail" || name == "tail-serial-port" || name == "tail_serial_port",
		}
	}
}

#[derive(Subcommand, Debug)]
pub enum MionSubcommands {
	/// Dump the EEPROM on a mion.
	#[command(name = "dump-eeprom", alias = "dump_eeprom")]
	DumpEeprom {
		// ///////////////////////////////////////////////////
		// Shared Flags for targeting a single bridge.
		// ///////////////////////////////////////////////////
		#[command(flatten)]
		bridge_config_flags: BridgeConfigurationFlags,
		#[command(flatten)]
		scan_flags: BridgeScanFlags,
		#[command(flatten)]
		target_flags: TargetBridgeFlags,
		#[arg(
			index = 1,
			help = "Search for a bridge with a particular name/ip/mac address.",
			long_help = "If you don't want to specify what type you're searching for with `--ip`, `--mac-address`, or `--name` you can just pass in a positional argument where we can guess"
		)]
		bridge_name_positional: Option<String>,
		// ///////////////////////////////////////////////////
		// Get only command flags.
		// ///////////////////////////////////////////////////
		#[arg(
			short = 'p',
			long = "output-path",
			alias = "output_path",
			help = "The path to output the dumped EEPROM.",
			long_help = "The path to the file to write the EEPROM dump."
		)]
		output_path: Option<PathBuf>,
	},
	/// Dump the Memory on a mion.
	#[command(name = "dump-memory", alias = "dump_memory")]
	DumpMemory {
		// ///////////////////////////////////////////////////
		// Shared Flags for targeting a single bridge.
		// ///////////////////////////////////////////////////
		#[command(flatten)]
		bridge_config_flags: BridgeConfigurationFlags,
		#[command(flatten)]
		scan_flags: BridgeScanFlags,
		#[command(flatten)]
		target_flags: TargetBridgeFlags,
		#[arg(
			index = 1,
			help = "Search for a bridge with a particular name/ip/mac address.",
			long_help = "If you don't want to specify what type you're searching for with `--ip`, `--mac-address`, or `--name` you can just pass in a positional argument where we can guess"
		)]
		bridge_name_positional: Option<String>,
		// ///////////////////////////////////////////////////
		// Get only command flags.
		// ///////////////////////////////////////////////////
		#[arg(
			short = 'p',
			long = "output-path",
			alias = "output_path",
			help = "The path to output the dumped memory of the MION.",
			long_help = "The path to the file to write the dumped memory of the MION."
		)]
		output_path: Option<PathBuf>,
		#[arg(
			short = 'r',
			long = "resume-at",
			alias = "resume_at",
			help = "The byte offset to resume reading at.",
			long_help = "The byte offset on the page to resume reading at, use debug logs of bridgectl to see where you are if you intend to resume."
		)]
		resume_at: Option<usize>,
	},
	/// Dump the firmware from the memory of the MION.
	///
	/// This is useful because doing a full memory dump takes _forever_ however
	/// the firmware files are reaalistically only the first couple MBs of memory
	/// always loaded at 0x0.
	///
	/// This dumps _just_ the first 3Mb of memory to get _just_ the firmware.
	#[command(
		name = "dump-firmware-from-memory",
		alias = "dump_firmware_from_memory"
	)]
	DumpFirmwareFromMemory {
		// ///////////////////////////////////////////////////
		// Shared Flags for targeting a single bridge.
		// ///////////////////////////////////////////////////
		#[command(flatten)]
		bridge_config_flags: BridgeConfigurationFlags,
		#[command(flatten)]
		scan_flags: BridgeScanFlags,
		#[command(flatten)]
		target_flags: TargetBridgeFlags,
		#[arg(
			index = 1,
			help = "Search for a bridge with a particular name/ip/mac address.",
			long_help = "If you don't want to specify what type you're searching for with `--ip`, `--mac-address`, or `--name` you can just pass in a positional argument where we can guess"
		)]
		bridge_name_positional: Option<String>,
		// ///////////////////////////////////////////////////
		// Get only command flags.
		// ///////////////////////////////////////////////////
		#[arg(
			short = 'p',
			long = "output-path",
			alias = "output_path",
			help = "The path to output the partial dumped memory.",
			long_help = "The path to the file to write the partial memory dump."
		)]
		output_path: Option<PathBuf>,
	},
	/// Decrypt a MION Firmware file.
	#[command(
		name = "decrypt-fw",
		visible_aliases = [
			"decrypt_fw",
			"decrypt-firmware",
			"decrypt_firmware",
			"dfw",
		],
	)]
	DecryptFirmware {
		#[arg(
			short = 'p',
			long = "output-path",
			alias = "output_path",
			help = "The path to output the decrypted firmware file.",
			long_help = "The path to output the decrypted firmware file, can also be specified with positional arguments rather than flags, or not at all."
		)]
		output_path_flag: Option<PathBuf>,
		#[arg(
			index = 1,
			help = "The firmware file to decrypt.",
			long_help = "The path to the mion fw that we will decrypt."
		)]
		firmware_path: PathBuf,
		#[arg(
			index = 2,
			help = "The path to write the decrypted firmware file.",
			long_help = "The path to write the decrypted firmware file, you can also use `--output-path`, `-o` to specify this rather than a positional argument, or not at all."
		)]
		output_path_positional: Option<PathBuf>,
	},
	/// Encrypt a MION Firmware file.
	#[command(
		name = "encrypt-fw",
		visible_aliases = [
			"encrypt_fw",
			"encrypt-firmware",
			"encrypt_firmware",
			"efw",
		],
	)]
	EncryptFirmware {
		#[arg(
			short = 'p',
			long = "output-path",
			alias = "output_path",
			help = "The path to output the encrypted firmware file.",
			long_help = "The path to output the encrypted firmware file, can also be specified with positional arguments rather than flags, or not at all."
		)]
		output_path_flag: Option<PathBuf>,
		#[arg(
			index = 1,
			help = "The firmware file to encrypt.",
			long_help = "The path to the mion fw that we will encrypt."
		)]
		firmware_path: PathBuf,
		#[arg(
			index = 2,
			help = "The path to write the encrypted firmware file.",
			long_help = "The path to write the encrypted firmware file, you can also use `--output-path`, `-o` to specify this rather than a positional argument, or not at all."
		)]
		output_path_positional: Option<PathBuf>,
	},
}
impl MionSubcommands {
	/// If this subcommand matches a particular name.
	#[allow(unused)]
	#[must_use]
	pub fn name_matches(&self, name: &str) -> bool {
		match self {
			Self::DumpEeprom {
				bridge_config_flags,
				scan_flags,
				target_flags,
				bridge_name_positional,
				output_path,
			} => name == "dump-eeprom" || name == "dump_eeprom",
			Self::DumpMemory {
				bridge_config_flags,
				scan_flags,
				target_flags,
				bridge_name_positional,
				output_path,
				resume_at,
			} => name == "dump-memory" || name == "dump_memory",
			Self::DumpFirmwareFromMemory {
				bridge_config_flags,
				scan_flags,
				target_flags,
				bridge_name_positional,
				output_path,
			} => name == "dump-firmware-from-memory" || name == "dump_firmware_from_memory",
			Self::DecryptFirmware {
				output_path_flag,
				firmware_path,
				output_path_positional,
			} => [
				"decrypt-fw",
				"decrypt_fw",
				"decrypt-firmware",
				"decrypt_firmware",
				"dfw",
			]
			.contains(&name),
			Self::EncryptFirmware {
				output_path_flag,
				firmware_path,
				output_path_positional,
			} => [
				"encrypt-fw",
				"encrypt_fw",
				"encrypt-firmware",
				"encrypt_firmware",
				"efw",
			]
			.contains(&name),
		}
	}
}

/// Arguments specific to targeting just a single bridge to lookup.
#[derive(Args, Debug)]
pub struct TargetBridgeFlags {
	#[arg(
		short = 'd',
		long = "default",
		help = "Target the default bridge in your configuration file.",
		long_help = "A way to tell us you just want to use the default bridge in your configuration file, regardless of anything else."
	)]
	default: bool,
	#[arg(
		long = "bridge-from-env",
		help = "Target the bridge specified in your environment variables.",
		long_help = "A way to tell us you just want to use the bridge specified from the environment variables, regardless of anything else."
	)]
	mochiato: bool,
	#[arg(
		short = 'i',
		long = "ip",
		help = "Target the bridge located at this IP.",
		long_help = "A way to tell us you want to use the bridge that is located at the IP that matches the value of this flag."
	)]
	search_ip: Option<Ipv4Addr>,
	#[arg(
		short = 'm',
		long = "mac-address",
		alias = "mac_address",
		help = "Target the bridge with this MAC Address.",
		long_help = "A way to tell us you want to use the bridge that has the mac address that matches the value of this flag.."
	)]
	search_mac: Option<String>,
	#[arg(
		short = 'n',
		long = "name",
		help = "Target the bridge with this name.",
		long_help = "A way to tell us you want to use the bridge that has the name that matches the value of this flag."
	)]
	search_name: Option<String>,
}
impl TargetBridgeFlags {
	/// If the user specified a non search style flag (use the default, or use
	/// mochiato).
	#[must_use]
	pub const fn not_search_flag_specified(&self) -> bool {
		self.default || self.mochiato
	}
	#[must_use]
	pub const fn target_default(&self) -> bool {
		self.default
	}
	#[must_use]
	pub const fn target_mochiato(&self) -> bool {
		self.mochiato
	}

	/// If a "search" style targeting was specified.
	#[must_use]
	pub const fn specified_bridge_search_flag(&self) -> bool {
		self.search_ip.is_some() || self.search_mac.is_some() || self.search_name.is_some()
	}
	#[must_use]
	pub const fn search_for_ip(&self) -> Option<Ipv4Addr> {
		self.search_ip
	}
	#[must_use]
	pub const fn search_for_mac_specified(&self) -> bool {
		self.search_mac.is_some()
	}
	#[must_use]
	pub fn search_for_mac(&self) -> Option<MacAddress> {
		self.search_mac
			.as_deref()
			.and_then(|data: &str| MacAddress::try_from(data).ok())
	}
	#[must_use]
	pub fn search_for_mac_raw(&self) -> Option<&str> {
		self.search_mac.as_deref()
	}
	#[must_use]
	pub fn search_for_name(&self) -> Option<&str> {
		self.search_name.as_deref()
	}
}
impl Display for TargetBridgeFlags {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		write!(
			fmt,
			"Search Flags (--ip: `{:?}`, --mac: `{:?}`, --name: `{:?}`), Non-Search Flags: (--default: `{}`, --bridge-from-env: `{}`)",
			self.search_ip, self.search_mac, self.search_name, self.default, self.mochiato,
		)
	}
}
const TARGET_BRIDGE_FLAG_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("search_for_ip"),
	NamedField::new("search_for_mac"),
	NamedField::new("search_for_name"),
	NamedField::new("dont_search_use_default"),
	NamedField::new("dont_search_use_env"),
];
impl Structable for TargetBridgeFlags {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"TargetBridgeFlags",
			Fields::Named(TARGET_BRIDGE_FLAG_FIELDS),
		)
	}
}
impl Valuable for TargetBridgeFlags {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			TARGET_BRIDGE_FLAG_FIELDS,
			&[
				Valuable::as_value(&self.search_ip.as_ref().map(|ip| format!("{ip}"))),
				Valuable::as_value(&self.search_mac),
				Valuable::as_value(&self.search_name),
				Valuable::as_value(&self.default),
				Valuable::as_value(&self.mochiato),
			],
		));
	}
}

/// Common flags that are present on multiple subcommands for managing the
/// bridge configuration.
///
/// For now this is just the single flag, but we keep it in here incase it
/// expands in the future.
#[derive(Args, Debug)]
pub struct BridgeConfigurationFlags {
	#[arg(
		long = "bridge-state-path",
		alias = "bridge_state_path",
		help = "The path to your bridge configuration, a.k.a. `bridge_env.ini`.",
		long_help = "If you do not wish to use the default location, the explicit path to your `bridge_env.ini` file that we should use."
	)]
	bridge_state_path: Option<PathBuf>,
}
impl BridgeConfigurationFlags {
	/// The location to the bridge state we should use.
	#[must_use]
	pub fn bridge_state_path(&self) -> Option<&PathBuf> {
		self.bridge_state_path.as_ref()
	}
}
impl Display for BridgeConfigurationFlags {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		write!(
			fmt,
			"Bridge Config Location Override Flag --bridge-state-path: `{:?}`",
			self.bridge_state_path,
		)
	}
}
const BRIDGE_CONFIGURATION_FLAG_FIELDS: &[NamedField<'static>] =
	&[NamedField::new("bridge_config_location_override")];
impl Structable for BridgeConfigurationFlags {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"BridgeConfigurationFlags",
			Fields::Named(BRIDGE_CONFIGURATION_FLAG_FIELDS),
		)
	}
}
impl Valuable for BridgeConfigurationFlags {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			BRIDGE_CONFIGURATION_FLAG_FIELDS,
			&[Valuable::as_value(
				&self
					.bridge_state_path
					.as_ref()
					.map(|pb| format!("{}", pb.display())),
			)],
		));
	}
}

/// Flags used for searching for a bridge either just a single one, or
/// multiple.
#[derive(Args, Debug)]
pub struct BridgeScanFlags {
	#[arg(
		global = true,
		long = "bridge-control-port-override",
		alias = "bridge_control_port_override",
		help = "A way to override the control port which should never be needed.",
		long_help = "Allow overriding the scanning port aka CONTROL port for finding cat-dev bridges."
	)]
	control_port_override: Option<u16>,
	#[arg(
		global = true,
		long = "scan-early-timeout-seconds",
		alias = "scan_early_timeout_seconds",
		help = "The amount of seconds to wait before bailing early when scanning for a bridge (by default this is 3).",
		long_help = "CAT-DEV's MUST respond to broadcasts within 10 seconds, but in reality most folks only have one cat-dev / non busy networks were they will respond faster, in this case it's generally better to exit early. How early we decide to exit is controlled by this variable."
	)]
	scan_timeout: Option<u64>,
}
impl BridgeScanFlags {
	#[must_use]
	pub const fn control_port_override(&self) -> Option<u16> {
		self.control_port_override
	}
	#[must_use]
	pub const fn scan_timeout_override(&self) -> Option<u64> {
		self.scan_timeout
	}
}
impl Display for BridgeScanFlags {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		write!(
			fmt,
			"Scan Flags: (`--bridge-control-port-override`: {:?}, `--scan-early-timeout-seconds`: {:?})",
			self.control_port_override, self.scan_timeout,
		)
	}
}
const BRIDGE_SCAN_FLAGS: &[NamedField<'static>] = &[
	NamedField::new("bridge_control_port_override"),
	NamedField::new("scan_early_timeout_seconds"),
];
impl Structable for BridgeScanFlags {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static("BridgeScanFlags", Fields::Named(BRIDGE_SCAN_FLAGS))
	}
}
impl Valuable for BridgeScanFlags {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			BRIDGE_SCAN_FLAGS,
			&[
				Valuable::as_value(&self.control_port_override),
				Valuable::as_value(&self.scan_timeout),
			],
		));
	}
}

/// Common flags that are present on multiple subcommands for managing the
/// fs emulation configuration settings.
#[allow(
	// This struct isn't created through APIs where having more explicit types
	// would be more beneficial.
	//
	// These are all CLI arguments, which are explicitly named differently, and
	// not close.
	clippy::struct_excessive_bools,
)]
#[derive(Args, Debug)]
pub struct FSEmulConfigurationFlags {
	#[arg(
		long = "cafe-dir",
		alias = "cafe_dir",
		help = "The root path to your cafe directory, this is usually `C:\\cafe_sdk`, or `/opt/cafe_sdk`.",
		long_help = "If you do not wish to use the default location, the explicit path to your cafe sdk directory, which should contains folders named `slc`/`mlc` under the `data` folder."
	)]
	cafe_dir: Option<PathBuf>,
	#[arg(
		long = "disable-csr",
		visible_aliases = [
			"disable_csr",
			"noCSR",
			"nocsr"
		],
		help = "Disable Combined Send/Recv for protocols that support it.",
		long_help = "If you do not wish to enable Combined Send/Recv for Sata, and other protocols that support it.",
	)]
	disable_csr: bool,
	#[arg(
		long = "disable-ffio",
		visible_aliases = [
			"disable_ffio",
			"noFFIO",
			"noffio"
		],
		help = "Disable Fast File I/O for protocols that support it.",
		long_help = "If you do not wish to enable Fast File I/O for Sata, and other protocols that support it.",
	)]
	disable_ffio: bool,
	#[arg(
		long = "disable-load-bearing-sleep-for-atapi",
		alias = "disable_load_bearing_sleep_for_atapi",
		help = "Disable a load-bearing sleep necessary for unpatched cat-devs to work.",
		long_help = "Disable a load-bearing sleep necessary for unpatched cat-devs to work. The official cat-dev MION's will ack packets, but then throw them away, because it hates us."
	)]
	disable_load_bearing_sleep_for_atapi: bool,
	#[arg(
		long = "disable-load-bearing-sleep-for-sdio",
		alias = "disable_load_bearing_sleep_for_sdio",
		help = "Disable a load-bearing sleep necessary for unpatched cat-devs to work.",
		long_help = "Disable a load-bearing sleep necessary for unpatched cat-devs to work. The official cat-dev MION's will ack packets, but then throw them away, because it hates us."
	)]
	disable_load_bearing_sleep_for_sdio: bool,
	#[arg(
		long = "disable-load-bearing-sleep-for-pcfs",
		alias = "disable_load_bearing_sleep_for_pcfs",
		help = "Disable a load-bearing sleep necessary for unpatched cat-devs to work.",
		long_help = "Disable a load-bearing sleep necessary for unpatched cat-devs to work. The official cat-dev MION's will ack packets, but then throw them away, because it hates us."
	)]
	disable_load_bearing_sleep_for_pcfs: bool,
	#[arg(
		long = "disable-real-removal",
		alias = "disable_real_removal",
		help = "Disable actually removing files from the filesystem.",
		long_help = "Disable all removal of files/folders/symlinks from the filesystem, just rename them. Incase you're curious about it."
	)]
	disable_real_removal: bool,
	#[arg(
		long = "fsemul-config-path",
		alias = "fsemul_config_path",
		help = "The path to your fsemul configuration, a.k.a. `fsemul.ini`.",
		long_help = "If you do not wish to use the default location, the explicit path to your `fsemul.ini` file that we should use."
	)]
	fsemul_config_path: Option<PathBuf>,
	#[arg(
		long = "prefer-fsemul-over-network",
		alias = "prefer_fsemul_over_network",
		help = "If we should prefer the `fsemul.ini` file over the web configuration.",
		long_help = "If we should prefer the `fsemul.ini` file over the web configuration, this makes us act like nintendo's tools, but also means your configuration needs to be up to date."
	)]
	prefer_fsemul_over_network: bool,
	#[arg(
		long = "force-unique-fds",
		visible_aliases = [
			"force_unique_fds",
			"counter-fds",
			"counter_fds"
		],
		help = "Force unique file descriptors for files from HostFilesystem.",
		long_help = "If you want an easier time debugging fds and your OS reuses them, set this flag to guarantee all files get unique fds.",
	)]
	force_unique_fds: bool,
	#[arg(
		long = "sata-wal-log",
		alias = "sata_wal_log",
		help = "Where we should create a SATA WAL log for all PCFS Sata requests.",
		long_help = "Where we should create a SATA WAL log for all PCFS Sata requests, not that this does carry increased memory, and CPU time needing to be spent."
	)]
	sata_wal_log: Option<PathBuf>,
}
impl FSEmulConfigurationFlags {
	#[must_use]
	pub fn cafe_dir(&self) -> Option<&PathBuf> {
		self.cafe_dir.as_ref()
	}

	#[must_use]
	pub const fn disable_csr(&self) -> bool {
		self.disable_csr
	}

	#[must_use]
	pub const fn disable_ffio(&self) -> bool {
		self.disable_ffio
	}

	#[must_use]
	pub const fn disable_load_bearing_sleep_for_atapi(&self) -> bool {
		self.disable_load_bearing_sleep_for_atapi
	}

	#[must_use]
	pub const fn disable_load_bearing_sleep_for_sdio(&self) -> bool {
		self.disable_load_bearing_sleep_for_sdio
	}

	#[must_use]
	pub const fn disable_load_bearing_sleep_for_pcfs(&self) -> bool {
		self.disable_load_bearing_sleep_for_pcfs
	}

	#[must_use]
	pub const fn disable_real_removal(&self) -> bool {
		self.disable_real_removal
	}

	#[must_use]
	pub fn fsemul_config_path(&self) -> Option<&PathBuf> {
		self.fsemul_config_path.as_ref()
	}

	#[must_use]
	pub const fn force_unique_fds(&self) -> bool {
		self.force_unique_fds
	}

	#[must_use]
	pub const fn prefer_fsemul_over_network(&self) -> bool {
		self.prefer_fsemul_over_network
	}

	#[must_use]
	pub fn sata_wal_log(&self) -> Option<&PathBuf> {
		self.sata_wal_log.as_ref()
	}
}
impl Display for FSEmulConfigurationFlags {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		write!(
			fmt,
			"FS Emulation Config Location Override Flag --fsemul-config-path: `{:?}`, --prefer-fsemul-over-network: `{}`, --cafe-dir: `{:?}`, --disable-csr: `{}`, --disable-ffio: `{}`, --disable-load-bearing-sleep-for-atapi: `{}`, --disable-load-bearing-sleep-for-pcfs: `{}`, --disable-load-bearing-sleep-for-sdio: `{}`, --disable-real-removal: `{}`, --force-unique-fds: `{}`, --sata-wal-log: `{:?}`",
			self.fsemul_config_path,
			self.prefer_fsemul_over_network,
			self.cafe_dir,
			self.disable_csr,
			self.disable_ffio,
			self.disable_load_bearing_sleep_for_atapi,
			self.disable_load_bearing_sleep_for_pcfs,
			self.disable_load_bearing_sleep_for_sdio,
			self.disable_real_removal,
			self.force_unique_fds,
			self.sata_wal_log,
		)
	}
}
const FSEMUL_CONFIGURATION_FLAG_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("fsemul_config_path"),
	NamedField::new("prefer_fsemul_over_network"),
	NamedField::new("cafe_dir"),
	NamedField::new("disable_ffio"),
	NamedField::new("disable_csr"),
	NamedField::new("disable_load_bearing_sleep_for_atapi"),
	NamedField::new("disable_load_bearing_sleep_for_pcfs"),
	NamedField::new("disable_load_bearing_sleep_for_sdio"),
	NamedField::new("disable_real_removal"),
	NamedField::new("force_unique_fds"),
	NamedField::new("sata_wal_log"),
];
impl Structable for FSEmulConfigurationFlags {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"FsEmulConfigurationFlags",
			Fields::Named(FSEMUL_CONFIGURATION_FLAG_FIELDS),
		)
	}
}
impl Valuable for FSEmulConfigurationFlags {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			FSEMUL_CONFIGURATION_FLAG_FIELDS,
			&[
				Valuable::as_value(
					&self
						.fsemul_config_path
						.as_ref()
						.map(|pb| format!("{}", pb.display())),
				),
				Valuable::as_value(&self.prefer_fsemul_over_network),
				Valuable::as_value(&self.cafe_dir.as_ref().map(|pb| format!("{}", pb.display()))),
				Valuable::as_value(&self.disable_ffio),
				Valuable::as_value(&self.disable_csr),
				Valuable::as_value(&self.disable_load_bearing_sleep_for_atapi),
				Valuable::as_value(&self.disable_load_bearing_sleep_for_pcfs),
				Valuable::as_value(&self.disable_load_bearing_sleep_for_sdio),
				Valuable::as_value(&self.disable_real_removal),
				Valuable::as_value(&self.force_unique_fds),
				Valuable::as_value(
					&self
						.sata_wal_log
						.as_ref()
						.map(|pb| format!("{}", pb.display())),
				),
			],
		));
	}
}

/// Common flags that are present on multiple subcommands for managing
/// shared server variables.
#[derive(Args, Debug)]
pub struct SharedServerFlags {
	#[arg(
		long = "atapi-port",
		alias = "atapi_port",
		help = "The port to bind our ATAPI server to.",
		long_help = "The port to bind our ATAPI server to, ATAPI is what managed raw HDD style reading."
	)]
	atapi_port: Option<u16>,
	#[arg(
		long = "bind-address",
		alias = "bind_address",
		help = "The address to bind too for the cat-dev to connect too.",
		long_help = "If you do not wish to use your local ip address, the ip address to bind servers too."
	)]
	bind_addr: Option<Ipv4Addr>,
	#[arg(
		long = "pcfs-sata-port",
		alias = "pcfs_sata_port",
		help = "The port to bind our PCFS Sata server to.",
		long_help = "The port to bind our PCFS Sata server to. PCFS Sata is where most of the core filesystem emulation."
	)]
	pcfs_sata_port: Option<u16>,
	#[arg(
		long = "sdio-control-port",
		alias = "sdio_control_port",
		help = "The port to bind our SDIO/Control server to.",
		long_help = "The port to bind our SDIO/Control server to. SDIO/Control is where the MION gets certain files that would be on internal eMMCs."
	)]
	sdio_control_port: Option<u16>,
	#[arg(
		long = "sdio-printf-port",
		alias = "sdio_printf_port",
		help = "The port to bind our SDIO/Printf server to.",
		long_help = "The port to bind our SDIO/Printf server to. SDIO/Printf is where certain log messages from the MION get dumped."
	)]
	sdio_printf_port: Option<u16>,
}
impl SharedServerFlags {
	#[must_use]
	pub const fn atapi_port(&self) -> Option<u16> {
		self.atapi_port
	}

	#[must_use]
	pub fn bind_addr(&self) -> Option<&Ipv4Addr> {
		self.bind_addr.as_ref()
	}

	#[must_use]
	pub const fn pcfs_sata_port(&self) -> Option<u16> {
		self.pcfs_sata_port
	}

	#[must_use]
	pub const fn sdio_control_port(&self) -> Option<u16> {
		self.sdio_control_port
	}

	#[must_use]
	pub const fn sdio_printf_port(&self) -> Option<u16> {
		self.sdio_printf_port
	}
}
impl Display for SharedServerFlags {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		write!(
			fmt,
			"Shared Server Flags --atapi-port: `{:?}`, --bind-addr: `{:?}`, --pcfs-sata-port: `{:?}`, --sdio-control-port: `{:?}`, --sdio-printf-port: `{:?}`",
			self.atapi_port,
			self.bind_addr,
			self.pcfs_sata_port,
			self.sdio_control_port,
			self.sdio_printf_port,
		)
	}
}
const SHARED_SERVER_FLAG_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("atapi_port"),
	NamedField::new("bind_addr"),
	NamedField::new("pcfs_sata_port"),
	NamedField::new("sdio_control_port"),
	NamedField::new("sdio_printf_port"),
];
impl Structable for SharedServerFlags {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SharedServerFlags",
			Fields::Named(SHARED_SERVER_FLAG_FIELDS),
		)
	}
}
impl Valuable for SharedServerFlags {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SHARED_SERVER_FLAG_FIELDS,
			&[
				Valuable::as_value(&self.atapi_port),
				Valuable::as_value(&self.bind_addr.map(|ip| format!("{ip}"))),
				Valuable::as_value(&self.pcfs_sata_port),
				Valuable::as_value(&self.sdio_control_port),
				Valuable::as_value(&self.sdio_printf_port),
			],
		));
	}
}

/// Common flags that are present on multiple subcommands for managing
/// serial-ports.
///
/// *note: there are some flags that may appear as a positional outside
/// of this.*
#[derive(Args, Debug)]
pub struct SharedSerialPortFlags {
	#[arg(
		short = 's',
		long = "serial-port-path",
		alias = "serial_port_path",
		help = "The path to the serial port to use (conflicts with the positional argument).",
		long_help = "The path to the serial port to use, on Windows you should use something like 'COM1', 'COM2', etc., on Linux this should be the full path to the device (conflicts with the positional argument)."
	)]
	serial_port_flag: Option<PathBuf>,
}
impl SharedSerialPortFlags {
	#[must_use]
	pub fn serial_port_flag(&self) -> Option<&PathBuf> {
		self.serial_port_flag.as_ref()
	}
}
impl Display for SharedSerialPortFlags {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		write!(
			fmt,
			"Shared Serial Port Flags --serial-port-path: `{:?}`",
			self.serial_port_flag,
		)
	}
}
const SHARED_SERIAL_PORT_FLAG_FIELDS: &[NamedField<'static>] =
	&[NamedField::new("serial_port_flag")];
impl Structable for SharedSerialPortFlags {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SharedSerialPortFlags",
			Fields::Named(SHARED_SERIAL_PORT_FLAG_FIELDS),
		)
	}
}
impl Valuable for SharedSerialPortFlags {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SHARED_SERIAL_PORT_FLAG_FIELDS,
			&[Valuable::as_value(
				&self
					.serial_port_flag
					.as_ref()
					.map(|p| format!("{}", p.display())),
			)],
		));
	}
}
