use crate::{
	commands::argv_helpers::{coalesce_serial_ports, spawn_serial_log_task},
	exit_codes::{TAIL_COULD_NOT_SPAWN, TAIL_NEEDS_SERIAL_PORT},
	utils::add_context_to,
};
use miette::miette;
use std::path::PathBuf;
use tracing::{error, field::valuable};

/// Tail a serial ports logs until a user manually hits Ctrl-C.
pub async fn handle_tail(
	use_json: bool,
	serial_port_flag: Option<PathBuf>,
	serial_port_positional: Option<PathBuf>,
) {
	let Some((serial_port, path)) = coalesce_serial_ports(
		use_json,
		serial_port_flag.as_ref(),
		serial_port_positional.as_ref(),
	) else {
		if use_json {
			error!(
				id = "bridgectl::tail::no_serial_port",
				help = valuable(&["You can use `bridgectl list-serial-ports` to get a list of serial ports you might be able to use."]),
				"Please specify a serial port to tail.",
			);
		} else {
			error!(
        "\n{:?}",
        add_context_to(
          miette!("No serial port specified to tail, needed a serial port to tail."),
          [
            miette!("You can specify a serial port with the argument without a flag, or through the flag `--serial-port-path` (aka `-s`)"),
            miette!("You can also set an environment variable: `BRIDGECTL_SERIAL_PORT` if you don't want to specify arguments."),
            miette!("On windows this should be a device name like `COM1`, `COM2`, etc., on Linux this should be a full path to a serial device like: `/dev/tty`"),
						miette!("You can get a full list of serial ports with `bridgectl list-serial-ports`."),
          ].into_iter(),
        ),
      );
		}

		std::process::exit(TAIL_NEEDS_SERIAL_PORT);
	};

	if let Err(cause) = spawn_serial_log_task(use_json, serial_port, path).await {
		if use_json {
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
