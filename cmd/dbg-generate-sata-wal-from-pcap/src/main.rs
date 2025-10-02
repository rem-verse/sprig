pub mod commands;
pub mod exit_codes;
pub mod knobs;

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
};
use clap::{Parser, error::ErrorKind as ClapErrorKind};
use rm_lisa::{
	display::{SuperConsole, SuperConsoleFlushGuard, renderers::JSONConsoleRenderer},
	initialize_logging, initialize_with_console,
};
use std::{
	io::{Stderr, Stdout},
	sync::Arc,
};
use tracing::{error, info};

#[tokio::main]
async fn main() {
	let (argv, _log_guard) = bootstrap_cli();

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
		error!(
			id = "dgswfp::help::internal",
			cause = "internal error: Failed to specify a single command, and didn't call `help` handler?"
		);

		std::process::exit(SHOULD_NEVER_HAPPEN_FAILURE);
	};

	match sub_command {
		Subcommands::GenerateSataWAL {
			sata_port,
			pcap,
			wal,
		} => {
			handle_generate(pcap, wal, sata_port).await;
		}
		// Help is handled above.
		Subcommands::Help {} => unreachable!(),
		Subcommands::GenerateSataPadlog {
			sata_port,
			pcap,
			padlog,
		} => {
			handle_padlog(&pcap, &padlog, sata_port).await;
		}
	}
}

fn bootstrap_cli() -> (CliArguments, SuperConsoleFlushGuard<Stdout, Stderr>) {
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

	let guard: SuperConsoleFlushGuard<Stdout, Stderr>;
	if use_json {
		match initialize_with_console(SuperConsole::new_preselected_renderers(
			"dgswfp",
			Arc::new(JSONConsoleRenderer::new()),
			Arc::new(JSONConsoleRenderer::new()),
		)) {
			Ok(console) => {
				guard = SuperConsoleFlushGuard::new(console);
			}
			Err(cause) => {
				println!(
					r#"{{"id": "dgswfp::logging::install_failure", "inner_display_error": "{}", "message": "Failed to install the logging handlers!"}}"#,
					format!("{cause:?}")
						.replace('"', "\\\"")
						.replace('\n', "  ")
				);

				std::process::exit(LOGGING_HANDLER_INSTALL_FAILURE);
			}
		}
	} else {
		match initialize_logging("dgswfp") {
			Ok(console) => {
				guard = SuperConsoleFlushGuard::new(console);
			}
			Err(cause) => {
				// We have to use a custom panic script here, because logging isn't setup yet.
				println!("Failed to install the logging handler to setup logging:\n{cause:?}");
				std::process::exit(LOGGING_HANDLER_INSTALL_FAILURE);
			}
		}
	}

	match args_opt {
		Ok(args) => (args, guard),
		Err(cause) => {
			if cause.kind() == ClapErrorKind::DisplayVersion {
				info!(
					id = "dgswfp::cli::print_version",
					version = format!(
						"{} ({})",
						format!("{}", cause.render()).trim(),
						option_env!("DGSWFP_BUILD").unwrap_or("unknown")
					),
				);

				std::process::exit(0);
			}

			error!(
				id = "dgswfp::cli::arg_parse_failure",
				error.kind = %cause.kind(),
				error.context = ?cause.context().map(|(kind, value)| format!("{kind}: {value}")).collect::<Vec<String>>(),
				error.rendered = %cause.render(),
				"Failed parsing CLI arguments"
			);

			std::process::exit(ARGV_PARSE_FAILURE);
		}
	}
}
