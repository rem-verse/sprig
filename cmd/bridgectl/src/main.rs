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
	commands::{
		argv_helpers::{initialize_host_bridge, initialize_scan_flags, target_bridge},
		handle_add_or_update, handle_boot, handle_dump_parameters, handle_get,
		handle_get_parameters, handle_help, handle_list, handle_list_serial_ports,
		handle_remove_bridge, handle_set_default_bridge, handle_set_parameters, handle_tail,
	},
	exit_codes::{
		ARGV_NO_COMMAND_SPECIFIED, ARGV_PARSE_FAILURE, LOGGING_HANDLER_INSTALL_FAILURE,
		SHOULD_NEVER_HAPPEN_FAILURE,
	},
	knobs::{
		cli::{CliArguments, Subcommands},
		env::USE_JSON_OUTPUT,
	},
};
use clap::Parser;
use log::install_logging_handlers;
use miette::miette;
use tracing::error;

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

#[allow(
	// Most of this is just farming out to subcommands which can't be shorter.
	clippy::too_many_lines,
)]
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
			bridge_config_flags,
			scan_flags,
			target_flags,
			bridge_name_positional,
			set_default,
		} => {
			initialize_host_bridge(bridge_config_flags).await;
			initialize_scan_flags(scan_flags).await;
			_ = target_bridge(target_flags, bridge_name_positional.as_deref(), false).await;

			handle_add_or_update(set_default).await;
		}
		Subcommands::Boot {
			bridge_config_flags,
			scan_flags,
			target_flags,
			bridge_name_positional,
			serial_port_flag,
			serial_port_positional,
			without_pcfs,
			take_ownership,
		} => {
			initialize_host_bridge(bridge_config_flags).await;
			initialize_scan_flags(scan_flags).await;
			_ = target_bridge(target_flags, bridge_name_positional.as_deref(), false).await;

			handle_boot(
				without_pcfs,
				(serial_port_flag, serial_port_positional),
				take_ownership,
			)
			.await;
		}
		Subcommands::DumpParameters {
			bridge_config_flags,
			scan_flags,
			target_flags,
			bridge_name_positional,
			parameter_space_port,
		} => {
			initialize_host_bridge(bridge_config_flags).await;
			initialize_scan_flags(scan_flags).await;
			_ = target_bridge(target_flags, bridge_name_positional.as_deref(), false).await;

			handle_dump_parameters(parameter_space_port).await;
		}
		Subcommands::Get {
			bridge_config_flags,
			scan_flags,
			target_flags,
			bridge_name_positional,
			output_as_table,
		} => {
			initialize_host_bridge(bridge_config_flags).await;
			initialize_scan_flags(scan_flags).await;
			_ = target_bridge(target_flags, bridge_name_positional.as_deref(), false).await;

			handle_get(output_as_table).await;
		}
		Subcommands::GetParameters {
			bridge_config_flags,
			scan_flags,
			target_flags,
			bridge_name_positional,
			parameter_space_port,
			parameter_names_positional,
		} => {
			initialize_host_bridge(bridge_config_flags).await;
			initialize_scan_flags(scan_flags).await;
			let used_first_arg = target_bridge(
				target_flags,
				bridge_name_positional.as_deref(),
				parameter_names_positional.is_some(),
			)
			.await;

			handle_get_parameters(
				bridge_name_positional,
				parameter_names_positional,
				parameter_space_port,
				used_first_arg,
			)
			.await;
		}
		// Help is handled above.
		Subcommands::Help {} => unreachable!(),
		Subcommands::List {
			bridge_config_flags,
			scan_flags,
			use_cache,
			output_as_table,
		} => {
			initialize_host_bridge(bridge_config_flags).await;
			initialize_scan_flags(scan_flags).await;
			handle_list(use_cache, output_as_table).await;
		}
		Subcommands::ListSerialPorts {} => {
			handle_list_serial_ports();
		}
		Subcommands::Remove {
			bridge_config_flags,
			scan_flags,
			target_flags,
			bridge_name_positional,
		} => {
			initialize_host_bridge(bridge_config_flags).await;
			initialize_scan_flags(scan_flags).await;
			_ = target_bridge(target_flags, bridge_name_positional.as_deref(), false).await;

			handle_remove_bridge().await;
		}
		Subcommands::SetDefault {
			bridge_config_flags,
			scan_flags,
			target_flags,
			bridge_name_positional,
		} => {
			initialize_host_bridge(bridge_config_flags).await;
			initialize_scan_flags(scan_flags).await;
			_ = target_bridge(target_flags, bridge_name_positional.as_deref(), false).await;

			handle_set_default_bridge().await;
		}
		Subcommands::SetParameters {
			bridge_config_flags,
			scan_flags,
			target_flags,
			bridge_name_positional,
			parameter_space_port,
			parameter_names_positional,
		} => {
			initialize_host_bridge(bridge_config_flags).await;
			initialize_scan_flags(scan_flags).await;
			let used_first_arg = target_bridge(
				target_flags,
				bridge_name_positional.as_deref(),
				parameter_names_positional.is_some(),
			)
			.await;

			handle_set_parameters(
				bridge_name_positional,
				parameter_names_positional,
				parameter_space_port,
				used_first_arg,
			)
			.await;
		}
		Subcommands::Tail {
			serial_port_flag,
			serial_port_positional,
		} => {
			handle_tail(serial_port_flag, serial_port_positional).await;
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

			std::process::exit(ARGV_PARSE_FAILURE);
		}
	}
}
