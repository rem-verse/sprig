//! Dump the first 3Mbs of memory for a running CAT-DEV.
//!
//! The firmware for the MION is always loaded at 0x0, and is never bigger
//! than a couple MBs (2.1Mb is the largest we know of, but we dump the first
//! 3 to be safe).

use crate::{
	commands::argv_helpers::get_targeted_bridge_ip,
	exit_codes::{DUMP_MEMORY_FAILURE, FAILED_TO_WRITE_TO_DISK},
	SHOULD_LOG_JSON,
};
use cat_dev::mion::cgis::dump_memory_with_writer;
use miette::miette;
use std::{
	fs::OpenOptions,
	io::{BufWriter, Write},
	path::PathBuf,
};
use tracing::{error, info};

/// How many bytes we should fetch before stopping fetching "firmware".
///
/// This corresponds to just past 3Mib. The largest firmware size we know of
/// takes up 2.1Mibs in memory, so this should _always_ be safe.
const EARLY_FW_STOP: usize = 3_146_240_usize;

/// Actual command handler for the `mion dump-firmware-from-memory` command.
pub async fn handle_dump_firmware_from_memory(output_path: Option<PathBuf>) {
	let bridge_ip = get_targeted_bridge_ip().await;
	if SHOULD_LOG_JSON() {
		info!(
			id = "bridgectl::mion::dump_firmware_from_memory::start",
			%bridge_ip,
			"Dumping MION FW from Memory, this may take a bit (a couple hours max)...",
		);
	} else {
		info!(
			%bridge_ip,
			"Dumping MION FW from Memory, this may take a bit (a couple hours max)...",
		);
	}

	let path = output_path.unwrap_or(PathBuf::from("88F6281-firmware-memory.bin"));
	let file_writer = match OpenOptions::new()
		.write(true)
		.append(false)
		.create(true)
		.truncate(true)
		.open(&path)
	{
		Ok(val) => val,
		Err(cause) => {
			if SHOULD_LOG_JSON() {
				error!(
				  id = "bridgectl::mion::dump_firmware_from_memory::write_failure",
				  %bridge_ip,
				  ?cause,
				  path = %path.to_string_lossy(),
				  "Failed to write 88F6281 memory to disk!",
				);
			} else {
				error!(
					"\n{:?}",
					miette!("Could not write successfully dumped MION's 88F6281 Memory")
						.wrap_err(cause),
				);
			}

			std::process::exit(FAILED_TO_WRITE_TO_DISK);
		}
	};
	let mut buff_writer = BufWriter::new(file_writer);

	if let Err(cause) =
		dump_memory_with_writer(bridge_ip, None, Some(EARLY_FW_STOP), |bytes: Vec<u8>| {
			if let Err(cause) = buff_writer.write(&bytes) {
				if SHOULD_LOG_JSON() {
					error!(
					  id = "bridgectl::mion::dump_firmware_from_memory::write_failure",
					  %bridge_ip,
					  ?cause,
					  path = %path.to_string_lossy(),
					  "Failed to write 88F6281 memory to disk!",
					);
				} else {
					error!(
						"\n{:?}",
						miette!("Could not write successfully dumped MION's 88F6281 Memory")
							.wrap_err(cause),
					);
				}

				std::process::exit(FAILED_TO_WRITE_TO_DISK);
			}
		})
		.await
	{
		if SHOULD_LOG_JSON() {
			error!(
			  id = "bridgectl::mion::dump_firmware_from_memory::failure",
			  %bridge_ip,
			  ?cause,
			  "Failure to dump MION's memory",
			);
		} else {
			error!(
				"\n{:?}",
				miette!("Could not dump MION's Memory.").wrap_err(cause),
			);
		}

		std::process::exit(DUMP_MEMORY_FAILURE);
	}
}
