//! Dump the memory for a running CAT-DEV.

use crate::{
	SHOULD_LOG_JSON,
	commands::argv_helpers::get_targeted_bridge_ip,
	exit_codes::{DUMP_MEMORY_FAILURE, FAILED_TO_WRITE_TO_DISK},
};
use cat_dev::mion::cgis::dump_memory_with_writer;
use miette::miette;
use std::{
	fs::OpenOptions,
	io::{BufWriter, Write},
	path::PathBuf,
};
use tracing::{error, info};

/// Actual command handler for the `mion dump-memory` command.
pub async fn handle_dump_memory(output_path: Option<PathBuf>, resume_at: Option<usize>) {
	let bridge_ip = get_targeted_bridge_ip().await;
	if SHOULD_LOG_JSON() {
		info!(
			id = "bridgectl::mion::dump_memory::start",
			%bridge_ip,
			"Dumping MION Memory, this will take a LONG time (like days...)...",
		);
	} else {
		info!(
			%bridge_ip,
			"Dumping MION Memory, this will take a LONG time (like days...)...",
		);
	}

	let path = output_path.unwrap_or(PathBuf::from("88F6281-memory.bin"));
	let file_writer = match OpenOptions::new()
		.write(true)
		.append(resume_at.is_some())
		.create(true)
		.open(&path)
	{
		Ok(val) => val,
		Err(cause) => {
			if SHOULD_LOG_JSON() {
				error!(
				  id = "bridgectl::mion::dump_memory::write_failure",
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

	if let Err(cause) = dump_memory_with_writer(bridge_ip, resume_at, None, |bytes: Vec<u8>| {
		if let Err(cause) = buff_writer.write(&bytes) {
			if SHOULD_LOG_JSON() {
				error!(
				  id = "bridgectl::mion::dump_memory::write_failure",
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
			  id = "bridgectl::mion::dump_memory::failure",
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
