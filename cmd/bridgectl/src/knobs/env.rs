//! The list of environment variables that influence behavior for `bridgectl`.

use once_cell::sync::Lazy;
use std::{
	env::{var as env_var, var_os as env_var_os},
	net::Ipv4Addr,
	path::PathBuf,
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
/// Type: [`PathBuf`].
pub static BRIDGE_HOST_STATE_PATH: Lazy<Option<PathBuf>> =
	Lazy::new(|| env_var_os("BRIDGECTL_BRIDGE_ENV_PATH").map(PathBuf::from));

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
