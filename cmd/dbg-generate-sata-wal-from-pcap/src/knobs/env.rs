//! The list of environment variables that influence behavior for `dbg-generate-sata-wal-from-pcap`.

use std::{env::var as env_var, sync::LazyLock};

/// Another way of configuring `dbg-generate-sata-wal-from-pcap` to output it's data in JSON.
///
/// Environment Variable Name: `DGSWFP_OUTPUT_JSON`
/// Expected Values: ("1" or "0"), and ("true" or "false")
/// Type: Boolean
pub static USE_JSON_OUTPUT: LazyLock<bool> =
	LazyLock::new(|| env_var("DGSWFP_OUTPUT_JSON").is_ok_and(|var| var == "1" || var == "true"));
