//! The list of environment variables that influence behavior for `bridgectl`.

use std::{
	env::{var as env_var, var_os as env_var_os},
	net::Ipv4Addr,
	path::PathBuf,
	sync::LazyLock,
	time::Duration,
};
use tracing::warn;

/// Another way of configuring `bridgectl` to output it's data in JSON.
///
/// Environment Variable Name: `BRIDGECTL_OUTPUT_JSON`
/// Expected Values: ("1" or "0"), and ("true" or "false")
/// Type: Boolean
pub static USE_JSON_OUTPUT: LazyLock<bool> =
	LazyLock::new(|| env_var("BRIDGECTL_OUTPUT_JSON").is_ok_and(|var| var == "1" || var == "true"));

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
pub static BRIDGE_CONTROL_PORT: LazyLock<Option<u16>> = LazyLock::new(|| {
	env_var("BRIDGE_CONTROL_PORT_OVERRIDE")
		.ok()
		.and_then(|val| match val.parse::<u16>() {
			Ok(val) => Some(val),
			Err(cause) => {
				warn!(
					?cause,
					"Not honoring environment variable `BRIDGE_CONTROL_PORT_OVERRIDE`, not a valid port number."
				);
				None
			}
		})
});

/// Set by `cafe`/`cafex`/`mochiato`, a way of specifying the bridge to
/// connect too.
///
/// Environment Variable Name: `BRIDGE_CURRENT_IP_ADDRESS`
/// Expected Values: Empty, or a String of a valid bridge ip address.
/// Type: [`Ipv4Addr`]
pub static BRIDGE_CURRENT_IP_ADDRESS: LazyLock<Option<Ipv4Addr>> = LazyLock::new(|| {
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

/// Set by `cafe`/`cafex`/`mochiato`, a way of specifying the bridge to
/// connect too.
///
/// Environment Variable Name: `BRIDGE_CURRENT_NAME`
/// Expected Values: Empty, or a String of a valid bridge name.
/// Type: String
pub static BRIDGE_CURRENT_NAME: LazyLock<Option<String>> =
	LazyLock::new(|| env_var("BRIDGE_CURRENT_NAME").ok());

/// A way of configuring the host address to listen on for servers.
///
/// note: We will always bind on the first networks IPv4 address unless this
/// is passed in.
///
/// Environment Variable Name: `BRIDGECTL_HOST`
/// Expected Values: Empty, or An IPV4 address.
/// Type: [`Ipv4Addr`]
pub static BRIDGECTL_HOST: LazyLock<Option<Ipv4Addr>> = LazyLock::new(|| {
	env_var("BRIDGECTL_HOST").ok().and_then(|val| {
		match val.parse::<Ipv4Addr>() {
			Ok(val) => Some(val),
			Err(cause) => {
				warn!(?cause, "Not honoring environment variable `BRIDGECTL_HOST`, not a valid IPv4 address (cat-dev requires IPv4).");
				None
			}
		}
	})
});

/// A way of specifying the path to the `bridge_env.ini` file if it's not in
/// a standard location.
///
/// Environment Variable Name: `BRIDGECTL_BRIDGE_ENV_PATH`
/// Expected Values: A Path
/// Type: [`PathBuf`]
pub static BRIDGE_HOST_STATE_PATH: LazyLock<Option<PathBuf>> =
	LazyLock::new(|| env_var_os("BRIDGECTL_BRIDGE_ENV_PATH").map(PathBuf::from));

/// A way of configuring the scan timeout rather than needing to manually
/// specify over the CLI. This value is specifically in seconds.
///
/// Environment Variable Name: `BRIDGE_SCAN_TIMEOUT_SECONDS`
/// Expected Values: Empty, or a number of seconds.
/// Type: [`u64`]
pub static BRIDGE_SCAN_TIMEOUT: LazyLock<Option<Duration>> = LazyLock::new(|| {
	env_var("BRIDGE_SCAN_TIMEOUT_SECONDS")
		.ok()
		.and_then(|val| match val.parse::<u64>() {
			Ok(val) => Some(Duration::from_secs(val)),
			Err(cause) => {
				warn!(
					?cause,
					"Not honoring environment variable `BRIDGE_SCAN_TIMEOUT_SECONDS`, not a valid number."
				);
				None
			}
		})
});

/// A way of specifying the serial port to read logs from so you don't have to
/// pass it in over a CLI flag.
///
/// Environment Variable Name: `BRIDGECTL_SERIAL_PORT`
/// Expected Values: `COM1`/`COM2`/etc. on Windows, `/dev/tty` on Linux.
/// Type: [`PathBuf`]
pub static BRIDGECTL_SERIAL_PORT: LazyLock<Option<PathBuf>> =
	LazyLock::new(|| env_var_os("BRIDGECTL_SERIAL_PORT").map(PathBuf::from));

/// The location of the root Cafe Directory.
///
/// Environment Variable Name: `CAFE_ROOT`
/// Expected Values: A Path
/// Type: [`PathBuf`]
pub static CAFE_ROOT: LazyLock<Option<PathBuf>> =
	LazyLock::new(|| env_var_os("CAFE_ROOT").map(PathBuf::from));

/// A way of configuring the timeout for connecting to a bridge.
///
/// note: there is always going to be a timeout the default is usually well
/// higher than we expect to ever see. However, we want folks to be able to
/// configure it themselves.
///
/// Environment Variable Name: `BRIDGECTL_CONNECTION_TIMEOUT_SECONDS`
/// Expected Values: Empty, or a number of seconds.
/// Type: [`u64`]
pub static CONNECTION_TIMEOUT: LazyLock<Option<Duration>> = LazyLock::new(|| {
	env_var("BRIDGECTL_CONNECTION_TIMEOUT_SECONDS").ok().and_then(|val| {
		match val.parse::<u64>() {
			Ok(val) => Some(Duration::from_secs(val)),
			Err(cause) => {
				warn!(?cause, "Not honoring environment variable `BRIDGECTL_CONNECTION_TIMEOUT_SECONDS`, not a valid second number.");
				None
			}
		}
	})
});

/// A way of specifying the path to the `fsemul.ini` file if it's not in
/// a standard location.
///
/// Environment Variable Name: `BRIDGECTL_FSEMUL_PATH`
/// Expected Values: A Path
/// Type: [`PathBuf`]
pub static FSMEUL_CONFIG_PATH: LazyLock<Option<PathBuf>> =
	LazyLock::new(|| env_var_os("BRIDGECTL_FSEMUL_PATH").map(PathBuf::from));

/// Determines if we should disable actually ever removing files for `FSEmul`.
///
/// Environment Variable Name: `DISABLE_REMOVAL_FOR_FSEMUL`
/// Expected Values: `1`, or `0` (`1` meaning true, `0` is default).
/// Type: [`bool`]
pub static FSEMUL_DISABLE_REMOVAL: LazyLock<bool> = LazyLock::new(|| {
	env_var("DISABLE_REMOVAL_FOR_FSEMUL")
		.ok()
		.as_deref()
		.unwrap_or("0")
		== "1"
});

/// Enables our ATAPI server to go "full throttle", and potentially overwhelm
/// the MION.
///
/// *YOU SHOULD NOT DISABLE LOAD BEARING SLEEP IF EVER TALKING TO A REAL, NON
/// MODIFIED CAT-DEV. YOU WILL EXPERIENCE BUGS. THE MION WILL ACK PACKETS BUT
/// NOT ACTUALLY PROCESS THEM.*
///
/// Environment Variable Name: `ATAPI_DISABLE_LOAD_BEARING_SLEEP`
/// Expected Values: `1`, or `0` (`1` meaning true, `0` the default).
/// Type: [`bool`]
pub static ATAPI_DISABLE_LOAD_BEARING_SLEEP: LazyLock<bool> = LazyLock::new(|| {
	env_var("ATAPI_DISABLE_LOAD_BEARING_SLEEP")
		.ok()
		.as_deref()
		.unwrap_or("0")
		== "1"
});

/// Determines if we should disable 'combined send/recv' for PCFS.
///
/// Environment Variable Name: `DISABLE_CSR_FOR_PCFS`
/// Expected Values: `1`, or `0` (`1` meaning true, `0` is default).
/// Type: [`bool`]
pub static PCFS_DISABLE_CSR: LazyLock<bool> = LazyLock::new(|| {
	env_var("DISABLE_CSR_FOR_PCFS")
		.ok()
		.as_deref()
		.unwrap_or("0")
		== "1"
});

/// Determines if we should disable 'fast file io' for PCFS.
///
/// Environment Variable Name: `DISABLE_FFIO_FOR_PCFS`
/// Expected Values: `1`, or `0` (`1` meaning true, `0` is default).
/// Type: [`bool`]
pub static PCFS_DISABLE_FFIO: LazyLock<bool> = LazyLock::new(|| {
	env_var("DISABLE_FFIO_FOR_PCFS")
		.ok()
		.as_deref()
		.unwrap_or("0")
		== "1"
});

/// Enables our PCFS server to go "full throttle", and potentially overwhelm
/// the MION.
///
/// *YOU SHOULD NOT DISABLE LOAD BEARING SLEEP IF EVER TALKING TO A REAL, NON
/// MODIFIED CAT-DEV. YOU WILL EXPERIENCE BUGS. THE MION WILL ACK PACKETS BUT
/// NOT ACTUALLY PROCESS THEM.*
///
/// Environment Variable Name: `PCFS_DISABLE_LOAD_BEARING_SLEEP`
/// Expected Values: `1`, or `0` (`1` meaning true, `0` the default).
/// Type: [`bool`]
pub static PCFS_DISABLE_LOAD_BEARING_SLEEP: LazyLock<bool> = LazyLock::new(|| {
	env_var("PCFS_DISABLE_LOAD_BEARING_SLEEP")
		.ok()
		.as_deref()
		.unwrap_or("0")
		== "1"
});

/// Determines if we are actively using "SATA" port for serving PCFS.
///
/// note: if this is set to false we will have to use SDIO for serving files
/// over PCFS. The "SDIO" protocol is not exactly great at serving large chunks
/// of files so we do recommend you keep this on.
///
/// Environment Variable Name: `USE_PCFS_OVER_SATA`
/// Expected Values: `1`, or `0` (`1` meaning true, the default).
/// Type: [`bool`]
pub static PCFS_IS_SATA: LazyLock<bool> =
	LazyLock::new(|| env_var("USE_PCFS_OVER_SATA").ok().as_deref().unwrap_or("1") == "1");

/// Enables our SDIO server to go "full throttle", and potentially overwhelm
/// the MION.
///
/// *YOU SHOULD NOT DISABLE LOAD BEARING SLEEP IF EVER TALKING TO A REAL, NON
/// MODIFIED CAT-DEV. YOU WILL EXPERIENCE BUGS. THE MION WILL ACK PACKETS BUT
/// NOT ACTUALLY PROCESS THEM.*
///
/// Environment Variable Name: `SDIO_DISABLE_LOAD_BEARING_SLEEP`
/// Expected Values: `1`, or `0` (`1` meaning true, `0` the default).
/// Type: [`bool`]
pub static SDIO_DISABLE_LOAD_BEARING_SLEEP: LazyLock<bool> = LazyLock::new(|| {
	env_var("SDIO_DISABLE_LOAD_BEARING_SLEEP")
		.ok()
		.as_deref()
		.unwrap_or("0")
		== "1"
});

/// An override to the Session Debug Port to connect to.
///
/// The Debug Out Port is just a simple replacement for a serial connection
/// when you don't have a physical connection to the cat-dev. It is truly just
/// a 1:1 mapping.
pub static SESSION_DEBUG_OUT_PORT: LazyLock<Option<u16>> = LazyLock::new(|| {
	env_var("SESSION_DEBUG_OUT_PORT")
		.ok()
		.and_then(|val| match val.parse::<u16>() {
			Ok(val) => Some(val),
			Err(cause) => {
				warn!(
					?cause,
					"Not honoring environment variable `SESSION_DEBUG_OUT_PORT`, not a valid port number."
				);
				None
			}
		})
});
