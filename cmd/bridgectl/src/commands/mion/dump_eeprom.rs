//! Dump the EEPROM for a running CAT-DEV.

use crate::{
	commands::argv_helpers::get_targeted_bridge_ip,
	exit_codes::{DUMP_EEPROM_FAILURE, FAILED_TO_WRITE_TO_DISK},
};
use cat_dev::mion::cgis::dump_eeprom;
use std::path::PathBuf;
use tokio::fs::write;
use tracing::{error, info};

/// Actual command handler for the `mion dump-eeprom` command.
pub async fn handle_dump_eeprom(output_path: Option<PathBuf>) {
	let bridge_ip = get_targeted_bridge_ip().await;
	info!(
		id = "bridgectl::mion::dump_eeprom::start",
		%bridge_ip,
		"Dumping MION EEPROM...",
	);

	match dump_eeprom(bridge_ip).await {
		Ok(memory) => {
			let err = if let Some(path) = output_path.as_ref() {
				write(path, memory).await.err()
			} else {
				write("eeprom-memory.bin", memory).await.err()
			};

			if let Some(cause) = err {
				error!(
				  id = "bridgectl::mion::dump_eeprom::write_failure",
				  %bridge_ip,
				  ?cause,
				  path = output_path.map_or("eeprom-memory.bin".to_owned(), |p| p.to_string_lossy().to_string()),
				  "Failed to write eeprom memory to disk!",
				);

				std::process::exit(FAILED_TO_WRITE_TO_DISK);
			}
		}
		Err(cause) => {
			error!(
			  id = "bridgectl::mion::dump_eeprom::open_failure",
			  %bridge_ip,
			  ?cause,
			  path = output_path.map_or("eeprom-memory.bin".to_owned(), |p| p.to_string_lossy().to_string()),
			  "Failed to open file where we planned on writing MION's EEPMROM too!",
			);

			std::process::exit(DUMP_EEPROM_FAILURE);
		}
	}
}
