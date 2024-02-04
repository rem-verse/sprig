#![allow(
	// I've always disliked this rule, most of the time imports are used WITHOUT
	// the module name, and the module name is only used in the top level import.
	//
	// Where this becomes significantly more helpful to read as it's out of
	// context.
	clippy::module_name_repetitions,
)]

mod knobs;
mod mionps_log;

use crate::{
	knobs::cli::CliOpts,
	mionps_log::{
		create_connection_established_logging_hook, create_read_finished_logging_hook,
		create_session_logging_hook, create_set_new_value_logging_hook,
		create_write_finished_logging_hook, create_write_set_finished_logging_hook,
		exit_async_error, exit_with_verbose_message, log_error, log_verbose,
	},
};
use cat_dev::{
	errors::{CatBridgeError, NetworkError, NetworkParseError},
	mion::{
		parameter::{get_parameters_with_logging_hooks, set_parameters_with_logging_hooks},
		proto::parameter::{well_known::ParameterLocationSpecification, DumpedMionParameters},
	},
};
use std::{env::args, net::Ipv4Addr};
use tokio::{runtime::Runtime, time::Duration};

fn main() {
	let opts = CliOpts::from(args().skip(1));
	if opts.ip_address.is_none() {
		CliOpts::print_help();
		std::process::exit(1);
	}

	if !opts.hex_dump && opts.offset.is_none() {
		if !opts.verbose && opts.timeout_ms.is_none() {
			CliOpts::print_help();
			std::process::exit(1);
		} else if opts.verbose && !opts.verbose_appeared_before_ip {
			// This returns an error message. Yes really.
			log_error("missing offset parameter");
			std::process::exit(9);
		}
	}

	if opts.verbose {
		log_verbose("Done parsing flags, will start socket subsystem");
	}
	let Ok(runtime) = Runtime::new() else {
		// 10091 -- this is a static errorcode for "system not ready", which is
		// probably the safest error.
		log_error("WSAStartup returned error WSAerror(10091)");
		exit_with_verbose_message(1, opts.verbose);
	};
	if opts.verbose {
		log_verbose("Socket subsystem started okay");
	}

	let Some(ip) = opts
		.ip_address
		.clone()
		.and_then(|val| val.parse::<Ipv4Addr>().ok())
	else {
		if opts.verbose {
			log_verbose("Got TCP Session");
			log_verbose("Using asynchronous mode, timeout=10000");
			log_verbose("tcpSessionFn returned 1");
		}

		log_error(&format!(
			"MIONTCPSession to ip {} returned error code 1",
			opts.ip_address.unwrap_or_default(),
		));
		exit_with_verbose_message(17, opts.verbose);
	};

	if opts.tried_to_set_value {
		let Some(offset) = opts.offset else {
			log_error("missing offset parameter");
			if opts.verbose {
				log_verbose("Returning early, error=9");
			}
			exit_with_verbose_message(9, opts.verbose);
		};
		let Some(large_value) = opts.set_to_value else {
			log_error("missing offset parameter");
			if opts.verbose {
				log_verbose("Returning early, error=9");
			}
			exit_with_verbose_message(9, opts.verbose);
		};
		let Ok(smol_value) = u8::try_from(large_value) else {
			log_error("invalid value (0-255)");
			if opts.verbose {
				log_verbose("Returning early, error=10");
			}
			exit_with_verbose_message(10, opts.verbose);
		};

		runtime.block_on(do_set(
			ip,
			opts.timeout_ms.map(Duration::from_millis),
			offset,
			smol_value,
			opts.verbose,
		));
	} else {
		let Ok(parameters) = runtime.block_on(get_parameters_with_logging_hooks(
			ip,
			opts.timeout_ms.map(Duration::from_millis),
			create_session_logging_hook(opts.verbose),
			create_connection_established_logging_hook(opts.verbose),
			create_write_finished_logging_hook(opts.verbose),
			create_read_finished_logging_hook(opts.verbose),
		)) else {
			exit_async_error(ip, opts.verbose);
		};

		if opts.hex_dump {
			do_dump(&parameters, opts.verbose);
		} else if let Some(offset) = opts.offset {
			do_single_value(ip, &parameters, offset, opts.verbose);
		}

		if opts.verbose {
			log_verbose("MionPsConnCallback returning 1007");
			log_verbose("tcpSessionFn returned 1007");
			log_verbose("Done with WSACleanup, about to return 0");
		}
	}
}

async fn do_set(ip: Ipv4Addr, timeout: Option<Duration>, offset: u16, value: u8, verbose: bool) {
	let (result, old_values) = match set_parameters_with_logging_hooks(
		vec![(ParameterLocationSpecification::Index(offset), value)].into_iter(),
		ip,
		timeout,
		create_session_logging_hook(verbose),
		create_connection_established_logging_hook(verbose),
		create_write_finished_logging_hook(verbose),
		create_read_finished_logging_hook(verbose),
		create_set_new_value_logging_hook(verbose),
		create_write_set_finished_logging_hook(verbose),
	)
	.await
	{
		Ok(successful_values) => successful_values,
		Err(cause) => {
			let size_error_code = match cause {
				CatBridgeError::NetworkError(NetworkError::ParseError(
					NetworkParseError::NotEnoughData(_name, _needed, got, _data),
				)) => i32::try_from(got).unwrap_or(i32::MAX),
				CatBridgeError::NetworkError(NetworkError::ParseError(
					NetworkParseError::UnexpectedTrailer(_name, trailer),
				)) => i32::try_from(trailer.len() + 12).unwrap_or(i32::MAX),
				CatBridgeError::NetworkError(NetworkError::ParseError(
					NetworkParseError::UnknownParamsPacketType(typ),
				)) => typ,
				CatBridgeError::NetworkError(NetworkError::ParseError(
					NetworkParseError::ParamsPacketErrorCode(ec),
				)) => ec,
				_ => 0,
			};

			println!("mionps: ERROR: set response returned error (status={size_error_code}/0)",);
			if verbose {
				log_verbose("tcpSessionFn returned 1010");
			}
			print!("mionps: ERROR: MIONTCPSession to ip {ip} returned error code 1010\n\nERROR!");
			exit_with_verbose_message(17, verbose);
		}
	};

	if result.is_success() {
		if verbose {
			log_verbose(&format!(
				"Success setting value(now:{value}/was:{}) at offset({offset}), Version untouched",
				old_values.get(&usize::from(offset)).unwrap_or(&0xFF),
			));
		}

		print!("{value}");

		if verbose {
			log_verbose("MionPsConnCallback returning 1008");
			log_verbose("tcpSessionFn returned 1008");
			log_verbose("Done with WSACleanup, about to return 0");
		}
	} else {
		println!(
			"mionps: ERROR: set response returned error (status=0/{})",
			result.get_return_code(),
		);
		if verbose {
			log_verbose("tcpSessionFn returned 1010");
		}
		print!("mionps: ERROR: MIONTCPSession to ip {ip} returned error code 1010\n\nERROR!");
		exit_with_verbose_message(17, verbose);
	}
}

fn do_dump(parameters: &DumpedMionParameters, verbose: bool) {
	if verbose {
		log_verbose("About to dump full space");
	}

	println!("Parameter Space dump:");
	for (chunk_idx, chunk) in parameters.get_raw_parameters().chunks(16).enumerate() {
		print!("{chunk_idx:02x}0: ");
		let mut ascii_str = String::with_capacity(16);
		for byte in chunk {
			print!("{byte:02x} ");
			let as_char = *byte as char;
			if as_char.is_ascii_alphanumeric() {
				ascii_str.push(as_char);
			} else {
				ascii_str.push('.');
			}
		}
		println!("    {ascii_str}");
	}
	println!();
}

fn do_single_value(ip: Ipv4Addr, parameters: &DumpedMionParameters, offset: u16, verbose: bool) {
	// Not getting a parameter by index _should_ be impossible since
	// the body is validated at this point to be of a certain size, and
	// we've validated the CLI argument.
	//
	// If this codepath every SOMEHOW ends up being taken, we should repsond
	// like if the real client didn't get enough parameters, which since the
	// original is a good tcp client it understands if it doesn't receive the
	// full body more may be coming, and thus it waits, and you hit a timeout
	// case.
	let Ok(param) = parameters.get_parameter_by_index(usize::from(offset)) else {
		exit_async_error(ip, verbose);
	};
	print!("{param}");
}
