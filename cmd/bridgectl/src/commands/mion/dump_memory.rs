//! Dump the memory for a running CAT-DEV.

use crate::{
	commands::argv_helpers::get_targeted_bridge_ip,
	exit_codes::{DUMP_MEMORY_FAILURE, FAILED_TO_WRITE_TO_DISK},
	SHOULD_LOG_JSON,
};
use cat_dev::mion::cgis::dump_memory;
use miette::miette;
use std::path::PathBuf;
use tokio::fs::write;
use tracing::{error, info};

/// Actual command handler for the `mion dump-memory` command.
pub async fn handle_dump_memory(output_path: Option<PathBuf>) {
	let bridge_ip = get_targeted_bridge_ip().await;
	if SHOULD_LOG_JSON() {
		info!(
			id = "bridgectl::mion::dump_memory::start",
			%bridge_ip,
			"Dumping MION Memory...",
		);
	} else {
		info!(
			%bridge_ip,
			"Dumping MION Memory...",
		);
	}

	match dump_memory(bridge_ip).await {
		Ok(memory) => {
			let err = if let Some(path) = output_path.as_ref() {
				write(path, memory).await.err()
			} else {
				write("88F6281-memory.bin", memory).await.err()
			};

			if let Some(cause) = err {
				if SHOULD_LOG_JSON() {
					error!(
					  id = "bridgectl::mion::dump_memory::write_failure",
					  %bridge_ip,
					  ?cause,
					  path = output_path.map_or("88F6281-memory.bin".to_owned(), |p| p.to_string_lossy().to_string()),
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
		}
		Err(cause) => {
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
}
