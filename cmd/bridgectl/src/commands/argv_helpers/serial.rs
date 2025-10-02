use crate::{
	exit_codes::{
		ARGV_SERIAL_CONFLICTING_ARGUMENTS, SERIAL_PORT_CONNECTION_FAILURE,
		SHOULD_NEVER_HAPPEN_FAILURE,
	},
	knobs::{cli::SharedSerialPortFlags, env::BRIDGECTL_SERIAL_PORT},
};
use cat_dev::serial::{AsyncSerialPort, SerialLines};
use std::{path::PathBuf, time::Duration};
use tokio::{
	io::{AsyncBufRead, BufReader},
	signal::ctrl_c as ctrl_c_signal,
	task::{Builder as TaskBuilder, JoinHandle},
	time::sleep,
};
use tracing::{Instrument, debug, error, error_span, field::valuable, info, warn};

/// Determines if a positional argument that could be a path, or something
/// not a path, should be interpreted as a path.
///
/// This is used for the `tail` command where you can specify a bridge search
/// parameter, OR a path to a serial port.
#[must_use]
pub fn should_interpret_arg_as_port_path(arg: Option<&String>) -> bool {
	if let Some(potential_path) = arg {
		let as_path = PathBuf::from(potential_path);
		// For linux & also some cases in windows, we just care that a path is
		//
		// 1. Absolute, so no files in the same directory with say the name of
		//    a bridge ip collide.
		// 2. Exists, on disk. Thus isn't just something that looks like a path.
		if as_path.is_absolute() && as_path.exists() {
			return true;
		}

		#[cfg(windows)]
		{
			// On windows ONLY,, a user may have specified a shortname to a serial
			// port like 'COM1', 'COM2', etc.
			//
			// This wouldn't be absolute by itself, but is still not a bridge name.
			// So do this special checck for windows short names.
			let mut serial_port_path = PathBuf::from(r"\\.");
			serial_port_path.push(potential_path);
			if serial_port_path.exists() {
				return true;
			}
		}
	}

	false
}

/// Coalesce all serial port arguments into a serial port.
///
/// ## Panics
///
/// - If conflicting arguments (conflicting arg + env do not panic) are specified
///   for a serial port.
/// - If we cannot open a handle/descriptor to the associated serial device.
pub async fn coalesce_serial_ports(
	serial_port_flags: &SharedSerialPortFlags,
	serial_port_positional: Option<&PathBuf>,
) -> SerialLogger {
	let arg_to_take =
		if serial_port_flags.serial_port_flag().is_some() && serial_port_positional.is_some() {
			error!(
				id = "bridgectl::argv::conflicting_serial_port_args",
				flags.serial_port = valuable(&serial_port_flags),
				args.serial_port = ?serial_port_positional,
				help = valuable(&[
					"You only need to specify a serial port in one way, either through an argument, or a flag.",
					"There is no such thing as multiple serial ports for the cat-dev.",
				]),
			);

			std::process::exit(ARGV_SERIAL_CONFLICTING_ARGUMENTS);
		} else if let Some(flag) = serial_port_flags.serial_port_flag() {
			flag
		} else if let Some(pos) = serial_port_positional {
			pos
		} else if let Some(env) = BRIDGECTL_SERIAL_PORT.as_ref() {
			env
		} else {
			return SerialLogger::honk_shoo();
		};

	let port = match AsyncSerialPort::new(arg_to_take) {
		Ok(port) => port,
		Err(cause) => {
			error!(
				id = "bridgectl::argv::serial_connection_failure",
				?cause,
				help = "Please file an issue if it's not clear with your serial device.",
				port = %arg_to_take.display(),
				"failed to connect to serial device specified"
			);

			std::process::exit(SERIAL_PORT_CONNECTION_FAILURE);
		}
	};

	SerialLogger::new_from_serial_port(port, arg_to_take.clone())
}

/// A logger capable of reading logs from a 'serial port' style logger.
pub struct SerialLogger {
	/// A wrapper around an actual physical connected serial port.
	///
	/// This is a tuple type of (Async Serial Port Stream, Path to Serial Port).
	/// This is the preferred way of getting logs off of a cat-dev because
	/// frankly it's cheaper.
	serial_port: Option<(AsyncSerialPort, PathBuf)>,
}

impl SerialLogger {
	/// Construct a logger based off of a serial port...
	#[must_use]
	pub const fn new_from_serial_port(port: AsyncSerialPort, path: PathBuf) -> Self {
		Self {
			serial_port: Some((port, path)),
		}
	}

	/// Construct a logger that does nothing but sleep...
	#[must_use]
	pub const fn honk_shoo() -> Self {
		Self { serial_port: None }
	}

	#[must_use]
	pub const fn has_serial_port(&self) -> bool {
		self.serial_port.is_some()
	}

	/// Spawn a task that will watch for logs....
	pub fn spawn_log_task(self) -> JoinHandle<()> {
		if let Some((port, port_path)) = self.serial_port {
			Self::spawn_serial_log_task(port, port_path)
		} else {
			Self::spawn_sleep_task()
		}
	}

	/// Spawn a task that just sleeps forever.
	#[allow(clippy::blocks_in_conditions)]
	fn spawn_sleep_task() -> JoinHandle<()> {
		match TaskBuilder::new()
			.name("bridgectl::serial_log::honk_shoo")
			.spawn(async move {
				loop {
					tokio::select! {
						() = sleep(Duration::from_secs(u64::MAX)) => {}
						_ = ctrl_c_signal() => {
							info!(
								id = "bridgectl::serial::honk_shoo_detected_ctrlc",
								"ctrl-c has been hit, shutting down empty serial logger!",
							);

							break;
						}
					}
				}
			}) {
			Ok(port) => port,
			Err(cause) => {
				warn!(
					id = "bridgectl::serial::debug_out_watcher_spawn_failure",
					?cause,
					"failed to spawn task to watch serial logs; internal",
				);

				std::process::exit(SHOULD_NEVER_HAPPEN_FAILURE);
			}
		}
	}

	/// Spawn a task that reads from a physical serial port over, and over again.
	#[allow(clippy::blocks_in_conditions)]
	fn spawn_serial_log_task(port: AsyncSerialPort, port_path: PathBuf) -> JoinHandle<()> {
		match TaskBuilder::new()
			.name("bridgectl::serial_log::watcher")
			.spawn(async move {
				Self::do_serial_read_loop(BufReader::new(port))
					.instrument(error_span!(
						"bridgectl::serial::watch_serial",
						serial.path = %port_path.display(),
					))
					.await;
			}) {
			Ok(port) => port,
			Err(cause) => {
				warn!(
					id = "bridgectl::serial::watcher_spawn_failure",
					?cause,
					"failed to spawn task to watch serial logs; internal",
				);

				std::process::exit(SHOULD_NEVER_HAPPEN_FAILURE);
			}
		}
	}

	/// Perform the actual looping of reading logs from a source until Ctrl-C is
	/// hit.
	async fn do_serial_read_loop<Ty: AsyncBufRead + Unpin>(source: Ty) {
		let mut reader = SerialLines::new(source);

		loop {
			tokio::select! {
				res = reader.next_line() => {
					match res {
						Ok(Some(line)) => info!(
							id = "bridgectl::serial_log::watcher::line",
							line,
						),
						Ok(None) => {
							debug!(
								id = "bridgectl::serial_log::watcher::graceful_shutdown",
								shutdown_reason = "empty-receive",
								"shutting down gracefully"
							);

							break;
						}
						Err(cause) => {
							warn!(
								id = "bridgectl::serial_log::watcher::failure",
								?cause,
								"could not receive lines from this serial port."
							);

							break;
						}
					}
				}
				_ = ctrl_c_signal() => {
					debug!(
						id = "bridgectl::serial_log::watcher::graceful_shutdown",
						shutdown_reason = "ctrl-c",
						"shutting down gracefully"
					);

					break;
				}
			}
		}
	}
}
