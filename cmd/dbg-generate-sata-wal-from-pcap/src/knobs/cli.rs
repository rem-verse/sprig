//! Defines the command line interface a.k.a. all the arguments & flags.

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[clap(disable_help_flag = true, disable_help_subcommand = true)]
#[command(
	about,
	author,
	name = "dbg-generate-sata-wal-from-pcap",
	propagate_version = true,
	version
)]
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
	/// Generate a SATA WAL Log.
	#[command(name = "generate", visible_alias = "g")]
	Generate {
		#[arg(
			short = 'p',
			long = "sata-port",
			visible_alias = "sata_port",
			default_value_t = 7500
		)]
		sata_port: u32,
		#[arg(
			index = 1,
			help = "The PCAPNG file to read.",
			long_help = "The PCAPNG to generate a SATA WAL log from."
		)]
		pcap: PathBuf,
		#[arg(
			index = 2,
			help = "The WAL file to write.",
			long_help = "The location where we should write the final WAL."
		)]
		wal: PathBuf,
	},
	/// An alternative to `-h`, or `--help` to show the help for the top level CLI.
	#[command(name = "help")]
	Help {},
	/// Generate a PADLOG.
	#[command(name = "padlog")]
	Padlog {
		#[arg(
			short = 'p',
			long = "sata-port",
			visible_alias = "sata_port",
			default_value_t = 7500
		)]
		sata_port: u32,
		#[arg(
			index = 1,
			help = "The PCAPNG file to read.",
			long_help = "The PCAPNG to generate a SATA WAL log from."
		)]
		pcap: PathBuf,
	},
}
impl Subcommands {
	/// If this subcommand matches a particular name.
	#[allow(unused)]
	#[must_use]
	pub fn name_matches(&self, name: &str) -> bool {
		match self {
			Self::Generate {
				sata_port,
				pcap,
				wal,
			} => name == "generate" || name == "g",
			Self::Help {} => name == "help",
			Self::Padlog { sata_port, pcap } => name == "padlog",
		}
	}
}
