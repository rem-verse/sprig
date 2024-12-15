use crate::{
	exit_codes::{
		ARGV_SERIAL_CONFLICTING_ARGUMENTS, SERIAL_PORT_CONNECTION_FAILURE,
		SHOULD_NEVER_HAPPEN_FAILURE,
	},
	knobs::{
		cli::SharedSerialPortFlags,
		env::{BRIDGECTL_SERIAL_PORT, SESSION_DEBUG_OUT_PORT},
	},
	utils::add_context_to,
	SHOULD_LOG_JSON,
};
use cat_dev::serial::{AsyncSerialPort, SerialLines};
use miette::miette;
use std::{net::Ipv4Addr, path::PathBuf, time::Duration};
use tokio::{
	io::{AsyncBufRead, BufReader},
	net::TcpStream,
	signal::ctrl_c as ctrl_c_signal,
	task::{Builder as TaskBuilder, JoinHandle},
};
use tracing::{debug, error, error_span, field::valuable, info, warn, Instrument};

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
pub fn coalesce_serial_ports(
	mion_ip: Ipv4Addr,
	serial_port_flags: &SharedSerialPortFlags,
	serial_port_positional: Option<&PathBuf>,
) -> SerialLogger {
	let arg_to_take = if serial_port_flags.serial_port_flag().is_some()
		&& serial_port_positional.is_some()
	{
		if SHOULD_LOG_JSON() {
			error!(
				id = "bridgectl::argv::conflicting_serial_port_args",
				flags.serial_port = valuable(&serial_port_flags),
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
								"Serial Port Flags: {serial_port_flags}",
							),
							"A CAT-DEV does not support multiple serial ports at the same time.",
						),
					].into_iter(),
				),
			);
		}

		std::process::exit(ARGV_SERIAL_CONFLICTING_ARGUMENTS);
	} else if let Some(flag) = serial_port_flags.serial_port_flag() {
		flag
	} else if let Some(pos) = serial_port_positional {
		pos
	} else if let Some(env) = BRIDGECTL_SERIAL_PORT.as_ref() {
		env
	} else {
		// Connect to the debug out port.
		return SerialLogger::new_from_debug_out_port(
			mion_ip,
			serial_port_flags
				.debug_out_port()
				.or(*SESSION_DEBUG_OUT_PORT)
				.unwrap_or(6001),
		);
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

	SerialLogger::new_from_serial_port(port, arg_to_take.clone())
}

/// A logger capable of reading logs from a 'serial port' style logger.
pub struct SerialLogger {
	/// A port to connect over `DEBUG_OUT` which just shoves serial logs as
	/// raw bytes.
	debug_out: Option<(Ipv4Addr, u16)>,
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
			debug_out: None,
			serial_port: Some((port, path)),
		}
	}

	/// Construct a logger based off a port to connect to `DEBUG_OUT`...
	#[must_use]
	pub const fn new_from_debug_out_port(mion_ip: Ipv4Addr, port: u16) -> Self {
		Self {
			debug_out: Some((mion_ip, port)),
			serial_port: None,
		}
	}

	/// Spawn a task that will watch for logs....
	pub fn spawn_log_task(self) -> JoinHandle<()> {
		if let Some((port, port_path)) = self.serial_port {
			Self::spawn_serial_log_task(port, port_path)
		} else if let Some((ip, port)) = self.debug_out {
			Self::spawn_debug_out_task(ip, port)
		} else {
			unreachable!("No way to construct a serial logger without debug_out, or serial_port")
		}
	}

	/// Spawn a task that reads serial logs from the debug out port over, and over.
	#[allow(clippy::blocks_in_conditions)]
	fn spawn_debug_out_task(ip: Ipv4Addr, port: u16) -> JoinHandle<()> {
		match TaskBuilder::new()
			.name("bridgectl::serial_log::debug_out_watcher")
			.spawn(async move {
				info!(
					id = "bridgectl::serial::debug_out::start_connection",
					"Connecting to MION DEBUG_OUT port..."
				);
				let stream = match tokio::time::timeout(
					Duration::from_secs(30),
					TcpStream::connect((ip, port)),
				)
				.await
				{
					Ok(Ok(stream)) => stream,
					Ok(Err(cause)) => {
						if SHOULD_LOG_JSON() {
							warn!(
								alternatives = valuable(&[
									"You can always connect a USB to Serial Adapter to your cat-dev to get logs consistently",
								]),
								id = "bridgectl::serial::debug_out::connection_failure",
								?cause,
								"we could not connect to the Cat-Dev to listen for serial logs over DEBUG_OUT",
							);
						} else {
							warn!(
								alternatives = valuable(&[
									"You can always connect a USB to Serial Adapter to your cat-dev to get logs consistently",
								]),
								?cause,
								"we could not connect to the Cat-Dev to listen for serial logs over DEBUG_OUT",
							);
						}

						return;
					}
					Err(cause) => {
						if SHOULD_LOG_JSON() {
							warn!(
								alternatives = valuable(&[
									"You can always connect a USB to Serial Adapter to your cat-dev to get logs consistently",
								]),
								id = "bridgectl::serial::debug_out::connection_failure",
								?cause,
								"we could not connect to the Cat-Dev to listen for serial logs over DEBUG_OUT",
							);
						} else {
							warn!(
								alternatives = valuable(&[
									"You can always connect a USB to Serial Adapter to your cat-dev to get logs consistently",
								]),
								?cause,
								"we could not connect to the Cat-Dev to listen for serial logs over DEBUG_OUT",
							);
						}

						return;
					}
				};

				info!(
					id = "bridgectl::serial::debug_out::setup_connection",
					"Connected to DEBUG_OUT! Now streaming logs..."
				);
				Self::do_serial_read_loop(BufReader::new(stream))
					.instrument(error_span!(
						"bridgectl::serial::watch_debug_out",
						debug_out.ip = %ip,
						debug_out.port = port,
					))
					.await;
			}) {
			Ok(port) => port,
			Err(cause) => {
				if SHOULD_LOG_JSON() {
					warn!(
						id = "bridgectl::serial::debug_out_watcher_spawn_failure",
						?cause,
						"failed to spawn task to watch serial logs; internal",
					);
				} else {
					warn!(
						?cause,
						"internal error: failed to spawn task to watch for debug out serial logs, serial logs will not be watched for.",
					);
				}

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
						Ok(Some(line)) => if SHOULD_LOG_JSON() {
							info!(
								id = "bridgectl::serial_log::watcher::line",
								%line,
								"received log line from serial port",
							);
						} else {
							info!(line);
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
	}
}
