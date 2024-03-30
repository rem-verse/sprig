use crate::{
	exit_codes::{
		ARGV_SERIAL_CONFLICTING_ARGUMENTS, SERIAL_PORT_CONNECTION_FAILURE,
		SHOULD_NEVER_HAPPEN_FAILURE,
	},
	knobs::env::BRIDGECTL_SERIAL_PORT,
	utils::add_context_to,
	SHOULD_LOG_JSON,
};
use cat_dev::serial::{AsyncSerialPort, SerialLines};
use miette::miette;
use std::path::PathBuf;
use tokio::{
	io::BufReader,
	signal::ctrl_c as ctrl_c_signal,
	task::{Builder as TaskBuilder, JoinHandle},
};
use tracing::{debug, error, field::valuable, info, warn};

/// Coalesce all serial port arguments into a serial port.
///
/// ## Panics
///
/// - If conflicting arguments (conflicting arg + env do not panic) are specified
///   for a serial port.
/// - If we cannot open a handle/descriptor to the associated serial device.
pub fn coalesce_serial_ports(
	serial_port_flag: Option<&PathBuf>,
	serial_port_positional: Option<&PathBuf>,
) -> Option<(AsyncSerialPort, PathBuf)> {
	let arg_to_take = if serial_port_flag.is_some() && serial_port_positional.is_some() {
		if SHOULD_LOG_JSON() {
			error!(
				id = "bridgectl::argv::conflicting_serial_port_args",
				flags.serial_port = ?serial_port_flag,
				args.serial_port = ?serial_port_positional,
				suggestions = valuable(&[
					"You only need to specify a serial port in one way, either through an argument, or a flag.",
					"There is no such thing as multiple serial ports for the cat-dev.",
				]),
			);
		} else {
			error!(
				"\n{:?}",
				add_context_to(
					miette!("Positional argument conflicts with flag arguments!"),
					[
						miette!("You only need to specify a serial port in one way, either through an argument, or a flag."),
						miette!(
							help = format!(
								"Serial Port Flag: `{:?}` / Positional Argument: `{:?}`",
								serial_port_flag,
								serial_port_positional,
							),
							"A CAT-DEV does not support multiple serial ports at the same time.",
						),
					].into_iter(),
				),
			);
		}

		std::process::exit(ARGV_SERIAL_CONFLICTING_ARGUMENTS);
	} else if let Some(flag) = serial_port_flag {
		flag
	} else if let Some(pos) = serial_port_positional {
		pos
	} else {
		BRIDGECTL_SERIAL_PORT.as_ref()?
	};

	let port = match AsyncSerialPort::new(arg_to_take) {
		Ok(port) => port,
		Err(cause) => {
			if SHOULD_LOG_JSON() {
				error!(
					id = "bridgectl::argv::serial_connection_failure",
					?cause,
					help = "Please file an issue if it's not clear with your serial device.",
					port = %arg_to_take.display(),
					"failed to connect to serial device specified"
				);
			} else {
				error!(
					"\n{:?}",
					add_context_to(
						miette!("{cause:?}"),
						[
							miette!("Failed to connect to specified serial device."),
							miette!(
								help = format!("Specified serial device is: {}", arg_to_take.display()),
								"Please file an issue if it's not clear why your OS is giving us an error.",
							),
						]
						.into_iter(),
					),
				);
			}

			std::process::exit(SERIAL_PORT_CONNECTION_FAILURE);
		}
	};

	Some((port, arg_to_take.clone()))
}

/// Spawn a task that reads from a serial port over, and over again.
#[allow(clippy::blocks_in_conditions)]
pub fn spawn_serial_log_task(port: AsyncSerialPort, port_path: PathBuf) -> JoinHandle<()> {
	let handle = match TaskBuilder::new()
		.name("bridgectl::serial_log::watcher")
		.spawn(async move {
			let mut reader = SerialLines::new(BufReader::new(port));

			loop {
				tokio::select! {
					res = reader.next_line() => {
						match res {
							Ok(Some(line)) => if SHOULD_LOG_JSON() {
								info!(
									id = "bridgectl::serial_log::watcher::line",
									port = %port_path.display(),
									%line,
									"received log line from serial port",
								);
							} else {
								info!(port = %port_path.display(), line);
							}
							Ok(None) => {
								if SHOULD_LOG_JSON() {
									debug!(
										id = "bridgectl::serial_log::watcher::graceful_shutdown",
										shutdown_reason = "empty-receive",
										"shutting down gracefully"
									);
								} else {
									debug!(
										shutdown_reason = "empty-receive",
										"shutting down serial log watcher gracefully..."
									);
								}

								break;
							}
							Err(cause) => {
								if SHOULD_LOG_JSON() {
									warn!(
										id = "bridgectl::serial_log::watcher::failure",
										?cause,
										"could not receive lines from this serial port."
									);
								} else {
									warn!(?cause, "serial port gave us an error trying to read from it.");
								}

								break;
							}
						}
					}
					_ = ctrl_c_signal() => {
						if SHOULD_LOG_JSON() {
							debug!(
								id = "bridgectl::serial_log::watcher::graceful_shutdown",
								shutdown_reason = "ctrl-c",
								"shutting down gracefully"
							);
						} else {
							debug!(
								shutdown_reason = "ctrl-c",
								"shutting down serial log watcher gracefully..."
							);
						}

						break;
					}
				}
			}
		}) {
		Ok(port) => port,
		Err(cause) => {
			if SHOULD_LOG_JSON() {
				warn!(
					id = "bridgectl::serial::watcher_spawn_failure",
					?cause,
					"failed to spawn task to watch serial logs; internal",
				);
			} else {
				warn!(
					?cause,
					"internal error: failed to spawn task to watch for serial logs, serial logs will not be watched for.",
				);
			}

			std::process::exit(SHOULD_NEVER_HAPPEN_FAILURE);
		}
	};

	handle
}
