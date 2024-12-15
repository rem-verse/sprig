//! Code for "PCFS", or the actual serving of a filesystem for the cat-dev.
//!
//! PCFS is the most common thing users are 'expecting' to interact with. It
//! provides a full filesystem to the cat-dev. The easiest way to think about
//! it might be like `FSEmul` provides access to raw block data, while `PCFS`
//! provides a filesystem ontop of that block data.
//!
//! It's main protocols implement things like `CreateDirectory`, `OpenFile`,
//! etc.

pub mod errors;
pub mod sata_proto;

use crate::{
	errors::{APIError, CatBridgeError, NetworkError},
	fsemul::HostFilesystem,
};
use local_ip_address::local_ip;
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4};
use tokio::net::TcpListener;

/// The default port to use for hosting the SATA Server.
pub const DEFAULT_PCFS_OVER_SATA_PORT: u16 = 7500_u16;

/// An implementation of a PCFS server speaking the Sata over PCFS protocol.
#[derive(Debug)]
pub struct PCFSSataServer<'fs> {
	/// The address we're actively bound and listening on.
	bound_address: SocketAddrV4,
	/// A pointer to our integration with the host filesystem.
	///
	/// This let's us read data from the cafe directory and otherwise.
	host_filesystem: &'fs HostFilesystem,
	/// The listener to serve traffic on.
	server: TcpListener,
}

impl<'fs> PCFSSataServer<'fs> {
	/// Create and bind a new PCFS Sata Server to a port on an address.
	///
	/// ## Errors
	///
	/// If we cannot bind to the request host ip address.
	pub async fn new(
		host_filesystem: &'fs HostFilesystem,
		address: Option<Ipv4Addr>,
		port: Option<u16>,
	) -> Result<Self, CatBridgeError> {
		let Some(ip) = address.or_else(|| {
			// This always returns an ipv4 address, but is still returning
			// an ip address for legacy reasons.
			local_ip().ok().map(|ip| match ip {
				IpAddr::V4(v4) => v4,
				IpAddr::V6(_v6) => unreachable!(),
			})
		}) else {
			return Err(APIError::NoHostIpFound.into());
		};

		let bound_address = SocketAddrV4::new(ip, port.unwrap_or(DEFAULT_PCFS_OVER_SATA_PORT));
		let server = TcpListener::bind(bound_address)
			.await
			.map_err(NetworkError::IO)?;

		Ok(Self {
			bound_address,
			host_filesystem,
			server,
		})
	}
}
