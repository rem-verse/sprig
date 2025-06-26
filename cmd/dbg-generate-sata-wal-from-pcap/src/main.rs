pub mod commands;
pub mod exit_codes;
pub mod knobs;
pub mod utils;

use crate::{
	commands::{handle_generate, handle_help, handle_padlog},
	exit_codes::{
		ARGV_NO_COMMAND_SPECIFIED, ARGV_PARSE_FAILURE, LOGGING_HANDLER_INSTALL_FAILURE,
		SHOULD_NEVER_HAPPEN_FAILURE,
	},
	knobs::{
		cli::{CliArguments, Subcommands},
		env::USE_JSON_OUTPUT,
	},
	utils::add_context_to,
};
use clap::{Error as ClapError, Parser, error::ErrorKind as ClapErrorKind};
use log::install_logging_handlers;
use miette::{IntoDiagnostic, miette};
use tracing::{error, info};

/// Whether or not we're logging in JSON.
static mut USE_JSON: bool = false;

/// Whether or not we should log in JSON.
///
/// Wrapper around the "unsafe" static mutable. This is guaranteed to be
/// safe as it's initialized as the very first thing to be used in the
/// program, and guaranteed to not change after that.
#[allow(non_snake_case)]
#[inline]
#[must_use]
pub fn SHOULD_LOG_JSON() -> bool {
	unsafe { USE_JSON }
}

#[tokio::main]
async fn main() {
	let (argv, use_json) = bootstrap_cli();
	unsafe {
		USE_JSON = use_json;
	}

	if argv.help || argv.commands.is_none() || matches!(argv.commands, Some(Subcommands::Help {})) {
		let should_error = !argv.help && argv.commands.is_none();
		handle_help(argv.commands);
		std::process::exit(if should_error {
			ARGV_NO_COMMAND_SPECIFIED
		} else {
			0
		});
	}

	let Some(sub_command) = argv.commands else {
		if use_json {
			error!(
				id = "dgswfp::help::internal",
				cause = "Didn't call help even when subcommands was none?"
			);
		} else {
			error!(
				"\n{:?}",
				miette!(
					"internal error: Failed to specify a single command, and didn't call `help` handler?"
				),
			);
		}
		std::process::exit(SHOULD_NEVER_HAPPEN_FAILURE);
	};

	match sub_command {
		Subcommands::Generate {
			sata_port,
			pcap,
			wal,
		} => {
			handle_generate(pcap, wal, sata_port).await;
		}
		// Help is handled above.
		Subcommands::Help {} => unreachable!(),
		Subcommands::Padlog { sata_port, pcap } => {
			handle_padlog(&pcap, sata_port);
		}
	}
}

fn bootstrap_cli() -> (CliArguments, bool) {
	let args_opt = CliArguments::try_parse();

	let use_json_cli = args_opt.as_ref().map_or_else(
		|_error| {
			let mut use_json = false;

			// Try to identify if the user is wanting to use JSON, even when argument
			// parsing itself fails.
			for arg in std::env::args() {
				if arg.as_str() == "-j" || arg.as_str() == "--json" {
					use_json = true;
					break;
				}
			}

			use_json
		},
		|args| args.json,
	);
	let use_json = *USE_JSON_OUTPUT || use_json_cli;

	if let Err(cause) = install_logging_handlers(use_json) {
		// We have to use a custom panic script here, because logging isn't setup yet.
		if use_json {
			println!(
				r#"{{"id": "dgswfp::logging::install_failure", "inner_display_error": "{}", "message": "Failed to install the logging handlers!"}}"#,
				format!("{cause:?}").replace('"', "\\\"")
			);
		} else {
			println!("Failed to install the logging handler to setup logging:\n{cause:?}");
		}
		std::process::exit(LOGGING_HANDLER_INSTALL_FAILURE);
	}

	match args_opt {
		Ok(args) => (args, use_json),
		Err(cause) => {
			if cause.kind() == ClapErrorKind::DisplayVersion {
				if use_json {
					info!(
						id = "dgswfp::cli::print_version",
						version = format!(
							"{} ({})",
							format!("{}", cause.render()).trim(),
							option_env!("DGSWFP_BUILD").unwrap_or("unknown")
						),
					);
				} else {
					info!(
						"{}",
						format!(
							"{} ({})",
							format!("{}", cause.render()).trim(),
							option_env!("DGSWFP_BUILD").unwrap_or("unknown")
						),
					);
				}

				std::process::exit(0);
			}

			if use_json {
				error!(
					id = "dgswfp::cli::arg_parse_failure",
					error.kind = %cause.kind(),
					error.context = ?cause.context().map(|(kind, value)| format!("{kind}: {value}")).collect::<Vec<String>>(),
					error.rendered = %cause.render(),
					"Failed parsing CLI arguments"
				);
			} else {
				error!(
					"\n{:?}",
					add_context_to(
						Err::<(), ClapError>(cause).into_diagnostic().unwrap_err(),
						[miette!("Failed parsing CLI arguments!")].into_iter(),
					),
				);
			}

			std::process::exit(ARGV_PARSE_FAILURE);
		}
	}
}
