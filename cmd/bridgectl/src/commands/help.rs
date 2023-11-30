//! Handles the help command, or when the help flag is specified on a
//! particular subcommand.
//!
//! We have to handle `help` ourselves, as opposed to FULLY relying on [`clap`]
//! so we can do things like printing the output in JSON.

use crate::knobs::cli::{CliArguments, Subcommands};
use clap::{Arg, Command, CommandFactory};
use tracing::{field::valuable, info};
use valuable::Valuable;

/// Actually process the help command.
///
/// ## Panics
///
/// - Techincally this function could panic if a subcommand could not be
///   matched on name alone, which should never happen.
pub fn handle_help(output_json: bool, opt_sub_command: Option<Subcommands>) {
	let mut top_level_command = CliArguments::command();
	let mut subcommands_as_command = Subcommands::command();

	if !output_json {
		if let Some(sub_command) = opt_sub_command {
			let mut subcommands_as_command = Subcommands::command();
			let my_command = subcommands_as_command
				.get_subcommands_mut()
				.find(|potential_command| sub_command.name_matches(potential_command.get_name()))
				.expect("internal error: recognized subcommand could not be matched on name?");
			info!("{}", my_command.render_long_help());
		} else {
			info!("{}", top_level_command.render_long_help());
		}
		return;
	}

	let (command, is_top_level) = if let Some(sub_command) = opt_sub_command {
		let my_command = subcommands_as_command
			.get_subcommands_mut()
			.find(|potential_command| sub_command.name_matches(potential_command.get_name()))
			.expect(r#"{{"id": "bridgectl::help::internal_error", "cause": "internal error: recognized subcommand could not be matched on name?"}}"#);
		(my_command, false)
	} else {
		(&mut top_level_command, true)
	};

	let args = command
		.get_arguments()
		.map(OwnedSubcommandOptionsHelpOutput::from)
		.collect::<Vec<_>>();
	let aliases = command
		.get_all_aliases()
		.map(ToOwned::to_owned)
		.collect::<Vec<_>>();
	let command_name = command.get_name().to_owned();
	let help = format!("{}", command.render_long_help());
	let options = command
		.get_opts()
		.map(OwnedSubcommandOptionsHelpOutput::from)
		.collect::<Vec<_>>();
	let positionals = command
		.get_positionals()
		.map(OwnedSubcommandOptionsHelpOutput::from)
		.collect::<Vec<_>>();
	let subcommands = command
		.get_subcommands_mut()
		.map(SubcommandHelpOutput::from)
		.collect::<Vec<_>>();

	info!(
		id = if is_top_level {
			"bridgectl::help::top_level".to_owned()
		} else {
			format!("bridgectl::help::{command_name}")
		},
		help.args = valuable(&args),
		help.aliases = valuable(&aliases),
		help.display_help_text = help,
		help.options = valuable(&options),
		help.positionals = valuable(&positionals),
		help.name = command_name,
		help.sub_commands = valuable(&subcommands),
	);
}

#[derive(Debug, Valuable)]
struct SubcommandHelpOutput<'data> {
	pub aliases: Vec<&'data str>,
	pub args: Vec<SubcommandOptionsHelpOutput<'data>>,
	pub has_subcommands: bool,
	pub help_output: String,
	pub name: &'data str,
	pub options: Vec<SubcommandOptionsHelpOutput<'data>>,
	pub positionals: Vec<SubcommandOptionsHelpOutput<'data>>,
}
impl<'data> From<&'data mut Command> for SubcommandHelpOutput<'data> {
	fn from(value: &'data mut Command) -> SubcommandHelpOutput<'data> {
		let help_output = format!("{}", value.render_help());
		let aliases = value.get_all_aliases().collect::<Vec<_>>();
		let has_subcommands = value.has_subcommands();
		let name = value.get_name();
		let args = value
			.get_arguments()
			.map(SubcommandOptionsHelpOutput::from)
			.collect::<Vec<_>>();
		let options = value
			.get_opts()
			.map(SubcommandOptionsHelpOutput::from)
			.collect::<Vec<_>>();
		let positionals = value
			.get_positionals()
			.map(SubcommandOptionsHelpOutput::from)
			.collect::<Vec<_>>();

		Self {
			aliases,
			args,
			has_subcommands,
			help_output,
			name,
			options,
			positionals,
		}
	}
}

#[derive(Debug, Valuable)]
struct SubcommandOptionsHelpOutput<'data> {
	pub aliases: Option<Vec<&'data str>>,
	pub default_values: Vec<String>,
	pub long_flag_name: Option<&'data str>,
	pub short_aliases: Option<Vec<char>>,
	pub short_flag_name: Option<char>,
	pub option_help: Option<String>,
}
impl<'data> From<&'data Arg> for SubcommandOptionsHelpOutput<'data> {
	fn from(option: &'data Arg) -> SubcommandOptionsHelpOutput<'data> {
		Self {
			aliases: option.get_all_aliases(),
			default_values: option
				.get_default_values()
				.iter()
				.map(|dv| format!("{}", dv.to_string_lossy()))
				.collect(),
			long_flag_name: option.get_long(),
			short_aliases: option.get_all_short_aliases(),
			short_flag_name: option.get_short(),
			option_help: option.get_long_help().map(|sstr| format!("{sstr}")),
		}
	}
}

#[derive(Debug, Valuable)]
struct OwnedSubcommandOptionsHelpOutput {
	pub aliases: Option<Vec<String>>,
	pub default_values: Vec<String>,
	pub long_flag_name: Option<String>,
	pub short_aliases: Option<Vec<char>>,
	pub short_flag_name: Option<char>,
	pub option_help: Option<String>,
}
impl<'data> From<&'data Arg> for OwnedSubcommandOptionsHelpOutput {
	fn from(option: &'data Arg) -> OwnedSubcommandOptionsHelpOutput {
		Self {
			aliases: option
				.get_all_aliases()
				.map(|value| value.into_iter().map(ToOwned::to_owned).collect::<Vec<_>>()),
			default_values: option
				.get_default_values()
				.iter()
				.map(|dv| format!("{}", dv.to_string_lossy()))
				.collect(),
			long_flag_name: option.get_long().map(ToOwned::to_owned),
			short_aliases: option.get_all_short_aliases(),
			short_flag_name: option.get_short(),
			option_help: option.get_long_help().map(|sstr| format!("{sstr}")),
		}
	}
}
