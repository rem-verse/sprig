//! Servers, and protocols related to ATAPI emulation.
//!
//! ATAPI Emulation is the actual hdd emulation parts of the CAT-DEV.
//! Unlike SDIO, ATAPI acts like EXI and initiates a connection from the CAT-DEV
//! to a host machine.
//!
//! This protocol much like SDIO though, ***IS NOT STANDARD***. I'm sorry if
//! you got a search request for ATAPI, and were looking for actual real ATAPI
//! code. Not this weird nintendo variant.

#[cfg(feature = "clients")]
pub mod client;
pub mod errors;
#[cfg(any(feature = "clients", feature = "servers"))]
pub mod proto;
#[cfg(feature = "servers")]
pub mod server;
