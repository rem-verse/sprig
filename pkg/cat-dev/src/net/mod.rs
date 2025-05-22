//! A series of "common" networking related utilities.
//!
//! These types, and structs, and logic aren't related to anything specific
//! to cat-dev's at all. But are simply utilities that allow us to actually
//! implement the rest of things like a "TCP Server", or "TCP Client".

pub mod additions;
#[cfg(feature = "clients")]
pub mod client;
pub mod errors;
mod ext_map;
pub mod handlers;
pub mod models;
#[cfg(feature = "servers")]
pub mod server;

pub use ext_map::Extensions;

#[cfg(test)]
use std::sync::RwLock;
use std::{
	env::var as env_var,
	sync::{LazyLock, atomic::AtomicU64},
	time::{Duration, SystemTime},
};

/// The default slowdown to use for cat-dev units that seems to work.
pub const DEFAULT_CAT_DEV_SLOWDOWN: Duration = Duration::from_millis(25);
/// The default amount to chunk write calls as to not overwhelm the cat-dev.
pub const DEFAULT_CAT_DEV_CHUNK_SIZE: usize = 8192;
/// The default "slow-loris" timeout or how long before we should error
/// because a connection is taking too long to send us a damn packet.
const DEFAULT_SLOWLORIS_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// An identifier for streams that increments globally.
static STREAM_ID: AtomicU64 = AtomicU64::new(1);
/// Determine how many bytes we should read in one "chunk" of a TCP Stream.
static TCP_READ_BUFFER_SIZE: usize = 8192_usize;

/// If we should be tracing IO regardless of what the user says.
static SPRIG_TRACE_IO: LazyLock<bool> = LazyLock::new(|| {
	if let Ok(variable) = env_var("SPRIG_FORCE_TRACE_ALL_IO") {
		variable != "0" && !variable.is_empty()
	} else {
		false
	}
});

thread_local! {
	#[cfg(test)]
	static CURRENT_TIME: LazyLock<RwLock<SystemTime>> = LazyLock::new(|| RwLock::new(SystemTime::now()));
}

#[cfg(not(test))]
fn now() -> SystemTime {
	SystemTime::now()
}

#[cfg(test)]
fn now() -> SystemTime {
	CURRENT_TIME.with(|time_lazy| time_lazy.read().expect("RwLock is poisioned?").clone())
}

#[cfg(test)]
pub mod test_helpers {
	#[cfg(feature = "servers")]
	pub use crate::net::server::test_helpers::*;
}
