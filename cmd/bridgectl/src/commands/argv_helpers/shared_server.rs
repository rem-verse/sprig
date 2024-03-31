//! Common wrapper around the shared server flag parameters.

use crate::knobs::{cli::SharedServerFlags, env::BRIDGECTL_HOST};
use std::net::Ipv4Addr;
use tokio::sync::RwLock;

static BOUND_ADDR: RwLock<Option<Ipv4Addr>> = RwLock::const_new(None);

/// Initialize all scanning flags.
pub async fn initialize_shared_server_flags(flags: SharedServerFlags) {
	if let Some(host) = BRIDGECTL_HOST.as_ref() {
		let mut addr_lock = BOUND_ADDR.write().await;
		*addr_lock = Some(*host);
	}

	if let Some(addr) = flags.bind_addr() {
		let mut addr_lock = BOUND_ADDR.write().await;
		*addr_lock = Some(*addr);
	}
}

/// Get the configured scan timeout for finding bridges.
#[must_use]
pub async fn get_host_bind_address() -> Option<Ipv4Addr> {
	let scan_lock = BOUND_ADDR.read().await;
	*scan_lock
}
