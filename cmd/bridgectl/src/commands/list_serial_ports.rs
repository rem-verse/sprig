//! Handling listing all the available serial ports known on your network.

use crate::exit_codes::{LSSP_FAILED_ENUMERATION, LSSP_NO_PORTS};
use cat_dev::serial::SyncSerialPort;
use tracing::{error, info};

pub fn handle_list_serial_ports() {
	let ports = match SyncSerialPort::available_ports() {
		Ok(ports) => ports,
		Err(cause) => {
			error!(
				id = "bridgectl::list_serial_ports::could_not_enumerate_ports",
				?cause,
				"failed to enumerate serial-ports",
			);

			std::process::exit(LSSP_FAILED_ENUMERATION);
		}
	};

	if ports.is_empty() {
		error!(
			id = "bridgectl::list_serial_ports::no_ports_found",
			"os returned 0 serial ports being found"
		);

		std::process::exit(LSSP_NO_PORTS);
	}

	for port in ports {
		info!(
			id = "bridgectl::list_serial_ports::found_port",
			port = %port.display(),
			"found a serial port",
		);
	}
}
