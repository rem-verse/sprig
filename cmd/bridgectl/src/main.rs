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
		handle_add_or_update, handle_boot, handle_dump_parameters, handle_get,
		handle_get_parameters, handle_help, handle_list, handle_list_serial_ports,
		handle_remove_bridge, handle_set_default_bridge, handle_set_parameters, handle_tail,
	},
	exit_codes::{
		ARGUMENT_PARSING_FAILURE, LOGGING_HANDLER_INSTALL_FAILURE, NO_ARGUMENT_SPECIFIED_FAILURE,
		SHOULD_NEVER_HAPPEN_FAILURE,
	},
	knobs::{
		cli::{CliArguments, Subcommands},
		env::USE_JSON_OUTPUT,
		get_control_port, get_scan_timeout,
	},
	utils::get_bridge_state_path,
};
use clap::Parser;
use log::install_logging_handlers;
use miette::miette;
use tracing::error;

#[allow(
	// Most of this is just farming out to subcommands which can't be shorter.
	clippy::too_many_lines,
)]
#[tokio::main]
async fn main() {
	let (argv, use_json) = bootstrap_cli();

	if argv.help || argv.commands.is_none() || matches!(argv.commands, Some(Subcommands::Help {})) {
		let should_error = !argv.help && argv.commands.is_none();
		handle_help(use_json, argv.commands);
		std::process::exit(if should_error {
			NO_ARGUMENT_SPECIFIED_FAILURE
		} else {
			0
		});
	}
	let scan_timeout = get_scan_timeout(&argv);
	let control_port = get_control_port(&argv);

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
				(scan_timeout, control_port),
				get_bridge_state_path(&argv.bridge_state_path, use_json),
				set_default,
			)
			.await;
		}
		Subcommands::Boot {
			default,
			bridge_ipaddr,
			bridge_mac,
			bridge_name,
			bridge_name_positional,
			without_pcfs,
			serial_port_flag,
			serial_port_positional,
		} => {
			handle_boot(
				use_json,
				default,
				(bridge_ipaddr, bridge_mac, bridge_name),
				bridge_name_positional,
				(scan_timeout, control_port),
				argv.bridge_state_path,
				without_pcfs,
				(serial_port_flag, serial_port_positional),
			)
			.await;
		}
		Subcommands::DumpParameters {
			default,
			bridge_ipaddr,
			bridge_mac,
			bridge_name,
			bridge_name_positional,
			parameter_space_port,
		} => {
			handle_dump_parameters(
				use_json,
				default,
				(bridge_ipaddr, bridge_mac, bridge_name),
				bridge_name_positional,
				(scan_timeout, control_port),
				parameter_space_port,
				argv.bridge_state_path,
			)
			.await;
		}
		Subcommands::GetParameters {
			default,
			bridge_ipaddr,
			bridge_mac,
			bridge_name,
			bridge_name_positional,
			parameter_names_positional,
			parameter_space_port,
		} => {
			handle_get_parameters(
				use_json,
				default,
				(bridge_ipaddr, bridge_mac, bridge_name),
				bridge_name_positional,
				parameter_names_positional,
				(scan_timeout, control_port),
				parameter_space_port,
				argv.bridge_state_path,
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
				(scan_timeout, control_port),
				argv.bridge_state_path,
			)
			.await;
		}
		// Help is handled above.
		Subcommands::Help {} => unreachable!(),
		Subcommands::List {
			use_cache,
			output_as_table,
		} => {
			handle_list(
				use_json,
				use_cache,
				output_as_table,
				(scan_timeout, control_port),
				argv.bridge_state_path,
			)
			.await;
		}
		Subcommands::ListSerialPorts {} => {
			handle_list_serial_ports(use_json);
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
		Subcommands::SetParameters {
			default,
			bridge_ipaddr,
			bridge_mac,
			bridge_name,
			bridge_name_positional,
			parameter_names_positional,
			parameter_space_port,
		} => {
			handle_set_parameters(
				use_json,
				default,
				(bridge_ipaddr, bridge_mac, bridge_name),
				bridge_name_positional,
				parameter_names_positional,
				(scan_timeout, control_port),
				parameter_space_port,
				argv.bridge_state_path,
			)
			.await;
		}
		Subcommands::Tail {
			serial_port_flag,
			serial_port_positional,
		} => {
			handle_tail(use_json, serial_port_flag, serial_port_positional).await;
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

			std::process::exit(ARGUMENT_PARSING_FAILURE);
		}
	}
}
