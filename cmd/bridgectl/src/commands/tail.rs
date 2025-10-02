use crate::{
	commands::argv_helpers::coalesce_serial_ports,
	exit_codes::{TAIL_COULD_NOT_SPAWN, TAIL_NO_SERIAL_PORT},
	knobs::cli::SharedSerialPortFlags,
};
use std::path::PathBuf;
use tracing::error;

/// Tail a serial ports, or debug out ports logs until a user manually hits Ctrl-C.
pub async fn handle_tail(
	serial_port_positional: Option<PathBuf>,
	serial_port_flags: SharedSerialPortFlags,
) {
	let serial_reader =
		coalesce_serial_ports(&serial_port_flags, serial_port_positional.as_ref()).await;

	if !serial_reader.has_serial_port() {
		error!(
			id = "bridgectl::tail::no_serial_port",
			"tail requires a serial port to connect too for now"
		);

		std::process::exit(TAIL_NO_SERIAL_PORT);
	}

	if let Err(cause) = serial_reader.spawn_log_task().await {
		error!(
			id = "bridgectl::tail::failed_to_join_task",
			?cause,
			"internal error: could not spawn/join task."
		);

		std::process::exit(TAIL_COULD_NOT_SPAWN);
	}
}
