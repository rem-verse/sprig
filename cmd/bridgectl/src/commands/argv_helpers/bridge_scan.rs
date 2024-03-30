//! Common wrapper around the bridge searching parameters.
//!
//! Some commands may need to execute scans for bridges across the network.
//! The goal of this package is to initialse shared arguments such as
//! the scan timeout, or scanning port.

use crate::knobs::{
	cli::BridgeScanFlags,
	env::{BRIDGE_CONTROL_PORT, BRIDGE_SCAN_TIMEOUT},
};
use cat_dev::mion::proto::DEFAULT_MION_CONTROL_PORT;
use std::time::Duration;
use tokio::sync::RwLock;

static SCAN_TIMEOUT: RwLock<Duration> = RwLock::const_new(Duration::from_secs(3));
static SCAN_CONTROL_PORT: RwLock<u16> = RwLock::const_new(DEFAULT_MION_CONTROL_PORT);

/// Initialize all scanning flags.
pub async fn initialize_scan_flags(flags: BridgeScanFlags) {
	if let Some(port) = BRIDGE_CONTROL_PORT.as_ref() {
		let mut port_lock = SCAN_CONTROL_PORT.write().await;
		*port_lock = *port;
	}
	if let Some(timeout) = BRIDGE_SCAN_TIMEOUT.as_ref() {
		let mut scan_lock = SCAN_TIMEOUT.write().await;
		*scan_lock = *timeout;
	}

	if let Some(cli_port) = flags.control_port_override() {
		let mut port_lock = SCAN_CONTROL_PORT.write().await;
		*port_lock = cli_port;
	}
	if let Some(timeout) = flags.scan_timeout_override() {
		let mut scan_lock = SCAN_TIMEOUT.write().await;
		*scan_lock = Duration::from_secs(timeout);
	}
}

/// Get the configured scan timeout for finding bridges.
#[must_use]
pub async fn get_scan_timeout() -> Duration {
	let scan_lock = SCAN_TIMEOUT.read().await;
	*scan_lock
}

/// Get the configured control port for scanning requests.
#[must_use]
pub async fn get_control_port() -> u16 {
	let scan_lock = SCAN_CONTROL_PORT.read().await;
	*scan_lock
}
