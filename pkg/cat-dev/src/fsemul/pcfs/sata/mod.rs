//! All the files related to the "SATA" protocol for PCFS.
//!
//! The custom sata protocol is the default for PCFS, and implements a real
//! filesystem such as "Create File"/"Open File"/etc.

#[cfg(feature = "clients")]
pub mod client;
#[cfg(any(feature = "clients", feature = "servers"))]
pub mod proto;
#[cfg(feature = "servers")]
pub mod server;
