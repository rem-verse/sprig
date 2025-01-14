//! Common wrapper around the shared server flag parameters.

use crate::knobs::{cli::SharedServerFlags, env::BRIDGECTL_HOST};
use std::net::Ipv4Addr;
use tokio::sync::RwLock;

static BOUND_ADDR: RwLock<Option<Ipv4Addr>> = RwLock::const_new(None);

static ATAPI_PORT: RwLock<Option<u16>> = RwLock::const_new(None);
static PCFS_SATA_PORT: RwLock<Option<u16>> = RwLock::const_new(None);
static SDIO_CONTROL_PORT: RwLock<Option<u16>> = RwLock::const_new(None);
static SDIO_PRINTF_PORT: RwLock<Option<u16>> = RwLock::const_new(None);

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

	if let Some(p) = flags.atapi_port() {
		let mut port_lock = ATAPI_PORT.write().await;
		*port_lock = Some(p);
	}
	if let Some(p) = flags.pcfs_sata_port() {
		let mut port_lock = PCFS_SATA_PORT.write().await;
		*port_lock = Some(p);
	}
	if let Some(p) = flags.sdio_control_port() {
		let mut port_lock = SDIO_CONTROL_PORT.write().await;
		*port_lock = Some(p);
	}
	if let Some(p) = flags.sdio_printf_port() {
		let mut port_lock = SDIO_PRINTF_PORT.write().await;
		*port_lock = Some(p);
	}
}

/// Get the configured address to bind on our host too.
#[must_use]
pub async fn get_host_bind_address() -> Option<Ipv4Addr> {
	let addr_lock = BOUND_ADDR.read().await;
	*addr_lock
}

/// Get the configured port to use for our ATAPI Server.
#[must_use]
pub async fn get_atapi_port() -> Option<u16> {
	let port_lock = ATAPI_PORT.read().await;
	*port_lock
}

/// Get the configured port to use for our ATAPI Server.
#[must_use]
pub async fn get_pcfs_sata_port() -> Option<u16> {
	let port_lock = PCFS_SATA_PORT.read().await;
	*port_lock
}

/// Get the configured port to use for our ATAPI Server.
#[must_use]
pub async fn get_sdio_control_port() -> Option<u16> {
	let port_lock = SDIO_CONTROL_PORT.read().await;
	*port_lock
}

/// Get the configured port to use for our ATAPI Server.
#[must_use]
pub async fn get_sdio_printf_port() -> Option<u16> {
	let port_lock = SDIO_PRINTF_PORT.read().await;
	*port_lock
}
