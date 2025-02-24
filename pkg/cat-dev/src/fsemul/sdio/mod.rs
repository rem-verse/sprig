//! Clients, and Protocols for interacting with the CAT-DEV's SDIO.
//!
//! Unlike traditional SDIO which is usually low-level firmware level protocol
//! that involves things like individual bits for the protocol, the CAT-DEV
//! takes a very 'interesting' approach to what they call SDIO.
//!
//! Specifically the first thing to note is that, "SDIO" can refer to two
//! ports on your CAT-DEV. There is "SDIO Printf/Control" (by default port
//! 7975), and "SDIO Block Data" (by default port 7976), which actually
//! interact over two totally independent TCP streams.

#[cfg(feature = "clients")]
pub mod client;
pub mod errors;
#[cfg(any(feature = "clients", feature = "servers"))]
pub mod proto;
#[cfg(feature = "servers")]
pub mod server;

use std::time::Duration;

/// The default port to use for "SDIO Printf/Control" communications.
///
/// It should be noted that a human can override this port in the MION itself.
/// However, most nintendo tools don't get this param from the MION itself, and
/// expect it to also be set in `fsemul.ini`.
pub const DEFAULT_SDIO_CONTROL_PORT: u16 = 7975;
/// The default port to use for "SDIO Block Data" communications.
///
/// It should be noted that a human can override this port in the MION itself.
/// However, most nintendo tools don't get this param from the MION itself, and
/// expect it to also be set in `fsemul.ini`.
pub const DEFAULT_SDIO_BLOCK_PORT: u16 = 7976;

/// The timeout to initiate a TCP connection to SDIO.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// The amount of TCP Packets that can be buffered per client.
///
/// *note: this is not the size of an individual packet, or packets, but is
/// just the amount of packets that can be queued.*
#[cfg(any(feature = "clients", feature = "servers"))]
const SDIO_TCP_PACKET_BUFFER_SIZE: usize = 8192_usize;
