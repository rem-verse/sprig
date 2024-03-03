//! The series of knobs that you can use to configure for `bridgectl`.
//!
//! NOTE: this doesn't include any flags potentially included in shared
//! libraries like those used for [`log`].

pub mod cli;
pub mod env;

use crate::knobs::{
	cli::CliArguments,
	env::{BRIDGE_CONTROL_PORT, BRIDGE_SCAN_TIMEOUT},
};
use cat_dev::mion::proto::DEFAULT_MION_CONTROL_PORT;
use std::time::Duration;

/// Get the configured scan timeout for finding bridges.
#[must_use]
pub fn get_scan_timeout(args: &CliArguments) -> Duration {
	let mut returned_timeout = args.scan_timeout.map(Duration::from_secs);
	if returned_timeout.is_none() {
		returned_timeout = *BRIDGE_SCAN_TIMEOUT;
	}
	returned_timeout.unwrap_or(Duration::from_secs(3))
}

/// Get the control port to use for scanning requests.
#[must_use]
pub fn get_control_port(args: &CliArguments) -> u16 {
	let mut returned_port = args.control_port_override;
	if returned_port.is_none() {
		returned_port = *BRIDGE_CONTROL_PORT;
	}
	returned_port.unwrap_or(DEFAULT_MION_CONTROL_PORT)
}
