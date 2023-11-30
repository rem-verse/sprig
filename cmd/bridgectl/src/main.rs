#![allow(
	// I've always disliked this rule, most of the time imports are used WITHOUT
	// the module name, and the module name is only used in the top level import.
	//
	// Where this becomes significantly more helpful to read as it's out of
	// context.
	clippy::module_name_repetitions,
)]

pub mod commands;
pub mod exit_codes;
pub mod knobs;
pub mod utils;

use crate::{
	commands::{handle_add_or_update, handle_get, handle_help, handle_list, handle_remove_bridge},
	exit_codes::{
		ARGUMENT_PARSING_FAILURE, LOGGING_HANDLER_INSTALL_FAILURE, NO_ARGUMENT_SPECIFIED_FAILURE,
		SHOULD_NEVER_HAPPEN_FAILURE,
	},
	knobs::{
		cli::{CliArguments, Subcommands},
		env::USE_JSON_OUTPUT,
	},
	utils::get_bridge_state_path,
};
use clap::Parser;
use commands::handle_set_default_bridge;
use log::install_logging_handlers;
use miette::miette;
use tracing::error;

#[tokio::main]
async fn main() {
	let (argv, use_json) = bootstrap_cli();

	if argv.help || argv.commands.is_none() || matches!(argv.commands, Some(Subcommands::Help {})) {
		let should_error = argv.commands.is_none();
		handle_help(use_json, argv.commands);
		std::process::exit(if should_error {
			NO_ARGUMENT_SPECIFIED_FAILURE
		} else {
			0
		});
	}

	let Some(sub_command) = argv.commands else {
		if use_json {
			error!(
				id = "bridgectl::help::internal",
				cause = "Didn't call help even when subcommands was none?"
			);
		} else {
			error!(
				"\n{:?}",
				miette!("internal error: Failed to specify a single command, and didn't call `help` handler?"),
			);
		}
		std::process::exit(SHOULD_NEVER_HAPPEN_FAILURE);
	};

	match sub_command {
		Subcommands::AddOrUpdate {
			bridge_name,
			bridge_ipaddr,
			bridge_name_positional,
			bridge_ip_positional,
			set_default,
		} => {
			handle_add_or_update(
				use_json,
				(bridge_name, bridge_ipaddr),
				(bridge_name_positional, bridge_ip_positional),
				get_bridge_state_path(&argv.bridge_state_path, use_json),
				set_default,
			)
			.await;
		}
		Subcommands::Get {
			default,
			bridge_ipaddr,
			bridge_mac,
			bridge_name,
			bridge_name_positional,
			output_as_table,
		} => {
			handle_get(
				use_json,
				output_as_table,
				default,
				(bridge_ipaddr, bridge_mac, bridge_name),
				bridge_name_positional,
				argv.bridge_state_path,
			)
			.await;
		}
		// Help is handled above.
		Subcommands::Help {} => unreachable!(),
		Subcommands::List {
			use_cache,
			scan_timeout,
			output_as_table,
		} => {
			handle_list(
				use_json,
				use_cache,
				output_as_table,
				scan_timeout,
				argv.bridge_state_path,
			)
			.await;
		}
		Subcommands::Remove {
			bridge_name,
			bridge_name_positional,
		} => {
			handle_remove_bridge(
				use_json,
				bridge_name,
				bridge_name_positional,
				argv.bridge_state_path,
			)
			.await;
		}
		Subcommands::SetDefault {
			bridge_name,
			bridge_name_positional,
		} => {
			handle_set_default_bridge(
				use_json,
				bridge_name,
				bridge_name_positional,
				argv.bridge_state_path,
			)
			.await;
		}
	}
}

fn bootstrap_cli() -> (CliArguments, bool) {
	let args_opt = CliArguments::try_parse();

	let use_json_cli = args_opt.as_ref().map_or_else(
		|_error| {
			let mut use_json = false;

			// Try to identify if the user is wanting to use JSON.
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
				r#"{{"id": "bridgectl::logging::install_failure", "inner_display_error": "{}", "message": "Failed to install the logging handlers!"}}"#,
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
			if use_json {
				error!(
					id = "bridgectl::cli::arg_parse_failure",
					error.kind = %cause.kind(),
					error.context = ?cause.context().map(|(kind, value)| format!("{kind}: {value}")).collect::<Vec<String>>(),
					error.rendered = %cause.render(),
					"Failed parsing CLI arguments"
				);
			} else {
				error!(
					"\n{:?}",
					miette!("Failed parsing CLI arguments!").wrap_err(cause),
				);
			}

			std::process::exit(ARGUMENT_PARSING_FAILURE);
		}
	}
}
