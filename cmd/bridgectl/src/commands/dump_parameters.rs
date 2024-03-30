use crate::{
	commands::argv_helpers::get_targeted_bridge_ip, exit_codes::DUMP_PARAMS_FAILED_TO_GET_PARAMS,
	SHOULD_LOG_JSON,
};
use cat_dev::mion::{parameter::get_parameters, proto::parameter::DumpedMionParameters};
use miette::miette;
use std::net::Ipv4Addr;
use tracing::{error, info};

/// Actual command handler for the `dump-parameters`, or `dp` command.
pub async fn handle_dump_parameters(parameter_space_port: Option<u16>) {
	let parameters = fetch_parameters(get_targeted_bridge_ip().await, parameter_space_port).await;

	if !SHOULD_LOG_JSON() {
		info!("\n\nDumping Parameter Space:");
	}

	for (chunk_idx, chunk) in parameters.get_raw_parameters().chunks(16).enumerate() {
		if SHOULD_LOG_JSON() {
			let mut ascii_str = String::with_capacity(16);
			let mut bytes = Vec::with_capacity(16);

			for byte in chunk {
				bytes.push(byte);
				let as_char = *byte as char;
				if as_char.is_ascii_alphanumeric() {
					ascii_str.push(as_char);
				} else {
					ascii_str.push('.');
				}
			}

			info!(
				id = "bridgectl::dump_parameters::dump_line",
				%ascii_str,
				?bytes,
			);
		} else {
			print!("  {chunk_idx:02x}0: ");
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
	}
}

async fn fetch_parameters(bridge_ip: Ipv4Addr, bridge_port: Option<u16>) -> DumpedMionParameters {
	match get_parameters(bridge_ip, bridge_port, None).await {
		Ok(params) => params,
		Err(cause) => {
			if SHOULD_LOG_JSON() {
				error!(
					id = "bridgectl::dump_parameters::failed_to_execute_dump_parameters",
					?cause,
					help = "We could not send/receive a packet to your MION to ask for it's parameters, please ensure it is running. If it's been running for awhile, it may need a reboot.",
				);
			} else {
				error!(
					"\n{:?}",
					miette!(
						help = "If you leave a MION running for too long it may stop responding to parameter requests.",
						"Could not send/receive a packet to your MION to ask for it's parameters, please ensure the device is running.",
					)
					.wrap_err(cause),
				);
			}
			std::process::exit(DUMP_PARAMS_FAILED_TO_GET_PARAMS);
		}
	}
}
