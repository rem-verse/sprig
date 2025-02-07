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

pub mod errors;
#[cfg(any(feature = "clients", feature = "servers"))]
pub mod proto;
#[cfg(feature = "servers")]
pub mod server;
