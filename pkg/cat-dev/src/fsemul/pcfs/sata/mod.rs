//! All the files related to the "SATA" protocol for PCFS.
//!
//! The custom sata protocol is the default for PCFS, and implements a real
//! filesystem such as "Create File"/"Open File"/etc.

#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
#[cfg(feature = "clients")]
pub mod client;
#[cfg_attr(docsrs, doc(cfg(any(feature = "clients", feature = "servers"))))]
#[cfg(any(feature = "clients", feature = "servers"))]
pub mod proto;
#[cfg_attr(docsrs, doc(cfg(feature = "servers")))]
#[cfg(feature = "servers")]
pub mod server;
