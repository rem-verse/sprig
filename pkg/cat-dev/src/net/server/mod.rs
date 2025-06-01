//! Common Network Related Server Utilities.
//!
//! This is guarded behind the servers feature flag, and is only necessary when
//! we are hosting servers of some variety.

pub mod models;
pub mod request_handlers;
pub mod requestable;
mod router;
mod tcp;

pub use router::Router;
pub use tcp::TCPServer;

#[cfg(test)]
pub mod test_helpers {
	pub use crate::net::server::router::test_helpers::*;
	pub use crate::net::server::tcp::test_helpers::*;
}
