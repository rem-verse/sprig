//! Handling listing all the available serial ports known on your network.

use crate::exit_codes::{LSSP_FAILED_ENUMERATION, LSSP_NO_PORTS};
use cat_dev::serial::SyncSerialPort;
use miette::miette;
use tracing::{error, info};

pub fn handle_list_serial_ports(use_json: bool) {
	let ports = match SyncSerialPort::available_ports() {
		Ok(ports) => ports,
		Err(cause) => {
			if use_json {
				error!(
					id = "bridgectl::list_serial_ports::could_not_enumerate_ports",
					?cause,
					"failed to enumerate serial-ports",
				);
			} else {
				error!(
					"\n{:?}",
					miette!(
						help = "Please report this issue for extra debugging support.",
						"OS error attempting to list serial ports",
					)
					.wrap_err(cause),
				);
			}

			std::process::exit(LSSP_FAILED_ENUMERATION);
		}
	};

	if ports.is_empty() {
		if use_json {
			error!(
				id = "bridgectl::list_serial_ports::no_ports_found",
				"os returned 0 serial ports being found"
			);
		} else {
			error!(
        "\n{:?}",
        miette!(
          help = "If you have a serial port device plugged in and recognized, please report an issue.",
          "Your OS returned 0 serial ports that were usable",
        )
      );
		}

		std::process::exit(LSSP_NO_PORTS);
	}

	for port in ports {
		if use_json {
			info!(
			  id = "bridgectl::list_serial_ports::found_port",
			  port = %port.display(),
			  "found a serial port",
			);
		} else {
			info!(
			  port = %port.display(),
			  "Found a usable serial port!",
			);
		}
	}
}
