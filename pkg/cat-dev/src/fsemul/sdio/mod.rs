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

#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
#[cfg(feature = "clients")]
pub mod client;
#[cfg_attr(docsrs, doc(cfg(any(feature = "clients", feature = "servers"))))]
#[cfg(any(feature = "clients", feature = "servers"))]
pub(crate) mod data_stream;
pub mod errors;
#[cfg_attr(docsrs, doc(cfg(any(feature = "clients", feature = "servers"))))]
#[cfg(any(feature = "clients", feature = "servers"))]
pub mod proto;
#[cfg_attr(docsrs, doc(cfg(feature = "servers")))]
#[cfg(feature = "servers")]
pub mod server;

use std::time::Duration;

#[cfg(any(feature = "clients", feature = "servers"))]
use scc::HashMap as ConcurrentHashMap;
#[cfg(any(feature = "clients", feature = "servers"))]
use std::sync::LazyLock;

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

/// Underlying 'data' streams for a series of <stream id, data stream>.
///
/// The data stream is truly just a data stream, all the benefits of having
/// this in a tcp client, or server, etc. just doesn't exist. It is an
/// arbitrary stream of bytes in ever since of the word.
///
/// This gets populated on SDIO Control stream start, and cleaned up on SDIO
/// Control stream end.
#[cfg(any(feature = "clients", feature = "servers"))]
static SDIO_DATA_STREAMS: LazyLock<ConcurrentHashMap<u64, data_stream::DataStream>> =
	LazyLock::new(|| ConcurrentHashMap::with_capacity(1));
