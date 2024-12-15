use crate::{
	commands::argv_helpers::{coalesce_serial_ports, get_targeted_bridge_ip},
	exit_codes::TAIL_COULD_NOT_SPAWN,
	knobs::cli::SharedSerialPortFlags,
	utils::add_context_to,
	SHOULD_LOG_JSON,
};
use miette::miette;
use std::path::PathBuf;
use tracing::error;

/// Tail a serial ports, or debug out ports logs until a user manually hits Ctrl-C.
pub async fn handle_tail(
	serial_port_positional: Option<PathBuf>,
	serial_port_flags: SharedSerialPortFlags,
) {
	let serial_reader = coalesce_serial_ports(
		get_targeted_bridge_ip().await,
		&serial_port_flags,
		serial_port_positional.as_ref(),
	);

	if let Err(cause) = serial_reader.spawn_log_task().await {
		if SHOULD_LOG_JSON() {
			error!(
				id = "bridgectl::tail::failed_to_join_task",
				?cause,
				"internal error: could not spawn/join task."
			);
		} else {
			error!(
				"\n{:?}",
				add_context_to(
					miette!("{cause:?}"),
					[miette!(
						"internal error: could not spawn/join tasks on a thread pool"
					)]
					.into_iter()
				),
			);
		}

		std::process::exit(TAIL_COULD_NOT_SPAWN);
	}
}
