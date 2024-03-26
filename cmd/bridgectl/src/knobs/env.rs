//! The list of environment variables that influence behavior for `bridgectl`.

use once_cell::sync::Lazy;
use std::{
	env::{var as env_var, var_os as env_var_os},
	net::Ipv4Addr,
	path::PathBuf,
	time::Duration,
};
use tracing::warn;

/// Another way of configuring `bridgectl` to output it's data in JSON.
///
/// Environment Variable Name: `BRIDGECTL_OUTPUT_JSON`
/// Expected Values: ("1" or "0"), and ("true" or "false")
/// Type: Boolean
pub static USE_JSON_OUTPUT: Lazy<bool> =
	Lazy::new(|| env_var("BRIDGECTL_OUTPUT_JSON").map_or(false, |var| var == "1" || var == "true"));

/// A way of specifying the path to the `bridge_env.ini` file if it's not in
/// a standard location.
///
/// Environment Variable Name: `BRIDGECTL_BRIDGE_ENV_PATH`
/// Expected Values: A Path
/// Type: [`PathBuf`]
pub static BRIDGE_HOST_STATE_PATH: Lazy<Option<PathBuf>> =
	Lazy::new(|| env_var_os("BRIDGECTL_BRIDGE_ENV_PATH").map(PathBuf::from));

/// A way of specifying the serial port to read logs from so you don't have to
/// pass it in over a CLI flag.
///
/// Environment Variable Name: `BRIDGECTL_SERIAL_PORT`
/// Expected Values: `COM1`/`COM2`/etc. on Windows, `/dev/tty` on Linux.
/// Type: [`PathBuf`]
pub static BRIDGECTL_SERIAL_PORT: Lazy<Option<PathBuf>> =
	Lazy::new(|| env_var_os("BRIDGECTL_SERIAL_PORT").map(PathBuf::from));

/// Set by `cafe`/`cafex`/`mochiato`, a way of specifying the bridge to
/// connect too.
///
/// Environment Variable Name: `BRIDGE_CURRENT_NAME`
/// Expected Values: Empty, or a String of a valid bridge name.
/// Type: String
pub static BRIDGE_CURRENT_NAME: Lazy<Option<String>> =
	Lazy::new(|| env_var("BRIDGE_CURRENT_NAME").ok());

/// Set by `cafe`/`cafex`/`mochiato`, a way of specifying the bridge to
/// connect too.
///
/// Environment Variable Name: `BRIDGE_CURRENT_IP_ADDRESS`
/// Expected Values: Empty, or a String of a valid bridge ip address.
/// Type: [`Ipv4Addr`]
pub static BRIDGE_CURRENT_IP_ADDRESS: Lazy<Option<Ipv4Addr>> = Lazy::new(|| {
	env_var("BRIDGE_CURRENT_IP_ADDRESS").ok().and_then(|val| {
		match val.parse::<Ipv4Addr>() {
			Ok(val) => Some(val),
			Err(cause) => {
				warn!(?cause, "Not Honoring `cafe`/`cafex`/`mochiato` set environment variable of `BRIDGE_CURRENT_IP_ADDRESS`, not a valid IPv4 address.");
				None
			}
		}
	})
});

/// A way of configuring the scan timeout rather than needing to manually
/// specify over the CLI. This value is specifically in seconds.
///
/// Environment Variable Name: `BRIDGE_SCAN_TIMEOUT_SECONDS`
/// Expected Values: Empty, or a number of seconds.
/// Type: [`u64`]
pub static BRIDGE_SCAN_TIMEOUT: Lazy<Option<Duration>> = Lazy::new(|| {
	env_var("BRIDGE_SCAN_TIMEOUT_SECONDS").ok().and_then(|val| {
		match val.parse::<u64>() {
			Ok(val) => Some(Duration::from_secs(val)),
			Err(cause) => {
				warn!(?cause, "Not honoring environment variable `BRIDGE_SCAN_TIMEOUT_SECONDS`, not a valid number.");
				None
			}
		}
	})
});

/// A way of configuring the port to reach out to a control port.
///
/// *note: we believe this port will ALWAYS be 7974, however, due to what we
/// believe is a buggy case there are some cases where official tools can
/// reach out to separate ports. AGAIN WE BELIEVE THIS IS A BUG, AND THUS YOU
/// SHOULD NEVER NEED TO CHANGE THIS. IF YOU DO, PLEASE CONTACT US SO WE CAN
/// DIG IN.*
///
/// Environment Variable Name: `BRIDGE_CONTROL_PORT_OVERRIDE`
/// Expected Values: Empty, or a port number (0-65536).
/// Type: [`u16`]
pub static BRIDGE_CONTROL_PORT: Lazy<Option<u16>> = Lazy::new(|| {
	env_var("BRIDGE_CONTROL_PORT_OVERRIDE").ok().and_then(|val| {
		match val.parse::<u16>() {
			Ok(val) => Some(val),
			Err(cause) => {
				warn!(?cause, "Not honoring environment variable `BRIDGE_CONTROL_PORT_OVERRIDE`, not a valid port number.");
				None
			}
		}
	})
});
