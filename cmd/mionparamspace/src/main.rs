#![allow(
	// I've always disliked this rule, most of the time imports are used WITHOUT
	// the module name, and the module name is only used in the top level import.
	//
	// Where this becomes significantly more helpful to read as it's out of
	// context.
	clippy::module_name_repetitions,
)]

mod knobs;

use crate::knobs::cli::CliOpts;
use cat_dev::{
	errors::{CatBridgeError, NetworkError, NetworkParseError},
	mion::{
		parameter::{get_parameters, set_parameters_and_get_changed_values},
		proto::parameter::{well_known::ParameterLocationSpecification, DumpedMionParameters},
	},
};
use std::{env::args, net::Ipv4Addr};
use tokio::runtime::Runtime;

fn main() {
	let opts = CliOpts::from(args().skip(1));

	if opts.too_many_parameters {
		println!("mionparamspace: ERROR: too many parameters");
		CliOpts::print_help();
		print!("ERROR!");
		std::process::exit(1);
	}
	if opts.mixed_args || opts.out_of_order {
		exit_invalid_ip_format();
	}

	let Some(string_ip) = opts.ip_address else {
		CliOpts::print_help();
		std::process::exit(1);
	};
	let Ok(ip) = string_ip.parse::<Ipv4Addr>() else {
		exit_invalid_ip_format();
	};

	if opts.dump {
		do_dump(ip);
	} else if !opts.tried_to_set {
		do_single(ip, opts.verbose, opts.set_to_value);
	} else {
		do_set(ip, opts.offset, opts.set_to_value, opts.verbose);
	}
}

fn do_set(ip: Ipv4Addr, potential_offset: Option<u64>, set_to_value: Option<u64>, verbose: bool) {
	let Some(offset) = potential_offset else {
		print!("mionparamspace: ERROR: missing offset parameter\nERROR!");
		std::process::exit(9);
	};
	if offset > 511 {
		print!("mionparamspace: ERROR: invalid offset (0-511)\nERROR!");
		std::process::exit(8);
	}
	// This should be impossible because of the size check above
	// so just fill in a bogus value if it's "too large".
	let smol_offset = u16::try_from(offset).unwrap_or(511);

	let Some(value) = set_to_value else {
		print!("mionparamspace: ERROR: missing value parameter\nERROR!");
		std::process::exit(11);
	};
	if value > 255 {
		print!("mionparamspace: ERROR: invalid value (0-255)\nERROR!");
		std::process::exit(10);
	}
	// This should be impossible because of the size check above
	// so just fill in a bogus value if it's "too large".
	let smol_value = u8::try_from(value).unwrap_or(u8::MAX);

	let runtime = create_runtime_or_exit(ip);
	match runtime
		.block_on(set_parameters_and_get_changed_values(
			vec![(
				ParameterLocationSpecification::Index(smol_offset),
				smol_value,
			)]
			.into_iter(),
			ip,
			None,
		))
		.map(|(resp, changed)| (resp.get_return_code(), changed))
	{
		Ok((0, changed)) => {
			if verbose {
				let octets = ip.octets();
				println!(
					"mionparamspace: Success setting value(now:{smol_value}/was:{}) at offset({}) on MION({:02}.{:02}.{:02}.{:02})",
					changed.get(&usize::from(smol_offset)).unwrap_or(&u8::MAX),
					// Yes... this tool really mixes up offsets/ips.
					octets[0],
					octets[1],
					octets[2],
					octets[3],
					smol_offset,
				);
			}
			print!("{smol_value}");
		}
		Ok((ec, _changed)) => {
			print!("mionparamspace: ERROR: get response data len error({ec})\nERROR!");
			std::process::exit(7);
		}
		Err(cause) => {
			let opt_error_code = match cause {
				CatBridgeError::NetworkError(NetworkError::ParseError(
					NetworkParseError::NotEnoughData(_name, _needed, got, _data),
				)) => i32::try_from(got).ok(),
				CatBridgeError::NetworkError(NetworkError::ParseError(
					NetworkParseError::UnexpectedTrailer(_name, trailer),
				)) => i32::try_from(trailer.len() + 12).ok(),
				CatBridgeError::NetworkError(NetworkError::ParseError(
					NetworkParseError::UnknownParamsPacketType(typ),
				)) => Some(typ),
				CatBridgeError::NetworkError(NetworkError::ParseError(
					NetworkParseError::ParamsPacketErrorCode(ec),
				)) => Some(ec),
				_ => None,
			};

			if let Some(ec) = opt_error_code {
				print!("mionparamspace: ERROR: get response data len error({ec})\nERROR!");
				std::process::exit(7);
			} else {
				// We don't have a particular error code, this is probably a bad
				// connection.
				exit_cant_connect(ip);
			}
		}
	}
}

fn do_single(ip: Ipv4Addr, verbose: bool, potential_offset: Option<u64>) {
	let Some(offset) = potential_offset else {
		print!("mionparamspace: ERROR: missing offset parameter\nERROR!");
		std::process::exit(9);
	};
	if offset > 511 {
		print!("mionparamspace: ERROR: invalid offset (0-511)\nERROR!");
		std::process::exit(8);
	}
	// This should be impossible because of the size check above
	// so just fill in a bogus value if it's "too large".
	let smol_offset = u16::try_from(offset).unwrap_or(511);
	let parameters = do_get_parameters(ip);
	// We've validated that this is within the space, this error should never
	// happen.
	let value = parameters
		.get_parameter_by_index(usize::from(smol_offset))
		.unwrap_or(u8::MAX);

	if verbose {
		println!("mionparamspace: value({value}) at offset({smol_offset}) on MION({ip})");
	}
	print!("{value}");
}

fn do_dump(ip: Ipv4Addr) {
	print_dump(&do_get_parameters(ip));
}

fn do_get_parameters(ip: Ipv4Addr) -> DumpedMionParameters {
	let runtime = create_runtime_or_exit(ip);
	match runtime.block_on(get_parameters(ip, None)) {
		Ok(val) => val,
		Err(cause) => {
			let opt_error_code = match cause {
				CatBridgeError::NetworkError(NetworkError::ParseError(
					NetworkParseError::NotEnoughData(_name, _needed, got, _data),
				)) => i32::try_from(got).ok(),
				CatBridgeError::NetworkError(NetworkError::ParseError(
					NetworkParseError::UnexpectedTrailer(_name, trailer),
				)) => i32::try_from(trailer.len() + 12).ok(),
				CatBridgeError::NetworkError(NetworkError::ParseError(
					NetworkParseError::UnknownParamsPacketType(typ),
				)) => Some(typ),
				CatBridgeError::NetworkError(NetworkError::ParseError(
					NetworkParseError::ParamsPacketErrorCode(ec),
				)) => Some(ec),
				_ => None,
			};

			if let Some(ec) = opt_error_code {
				print!("mionparamspace: ERROR: get response data len error({ec})\nERROR!");
				std::process::exit(7);
			} else {
				// We don't have a particular error code, this is probably a bad
				// connection.
				exit_cant_connect(ip);
			}
		}
	}
}

fn print_dump(parameters: &DumpedMionParameters) {
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

fn create_runtime_or_exit(ip: Ipv4Addr) -> Runtime {
	match Runtime::new() {
		Ok(runtime) => runtime,
		Err(_cause) => {
			exit_cant_connect(ip);
		}
	}
}

fn exit_cant_connect(ip: Ipv4Addr) -> ! {
	// Just pretend like we couldn't connect.
	let octets = ip.octets();
	print!(
		"mionparamspace: ERROR: could not connect to IP={:02}.{:02}.{:02}.{:02} errno=0x74\nERROR!",
		octets[0], octets[1], octets[2], octets[3],
	);
	std::process::exit(4);
}

fn exit_invalid_ip_format() -> ! {
	print!("mionparamspace: ERROR: IP address format not correct\nERROR!");
	std::process::exit(2);
}
