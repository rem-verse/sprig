//! Server implementation for SATA Server.

mod change_mode;
mod change_owner;
mod close_file;
mod close_folder;
mod connection_flags;
mod create_folder;
mod info_by_query;
mod open_file;
mod open_folder;
mod ping;
mod read_file;
mod read_folder;
mod remove;
mod rewind_folder;
mod write_file;

use crate::{
	errors::{APIError, CatBridgeError},
	fsemul::{
		HostFilesystem,
		pcfs::sata::{
			proto::SataRequest,
			server::connection_flags::{
				SATA_CONNECTION_FLAGS, SataConnectionFlags, SataConnectionFlagsLayer,
			},
		},
	},
	net::{
		DEFAULT_CAT_DEV_CHUNK_SIZE, DEFAULT_CAT_DEV_SLOWDOWN,
		additions::{RequestIDLayer, StreamIDLayer},
		models::{Endianness, FromRef, NagleGuard, Response},
		server::{Router, TCPServer, models::ResponseStreamEvent, requestable::Body},
	},
};
use bytes::Bytes;
use local_ip_address::local_ip;
use std::{
	net::{IpAddr, Ipv4Addr, SocketAddrV4},
	time::Duration,
};
use tower::ServiceBuilder;
use tracing::{field::valuable, warn};
use valuable::Valuable;

/// The default port to use for hosting the SATA Server.
pub const DEFAULT_SATA_PORT: u16 = 7500_u16;

/// The 'state' of the server that is server wide.
#[derive(Clone, Debug, Valuable)]
pub struct PCFSServerState {
	disable_real_removal: bool,
	host_filesystem: HostFilesystem,
	pid: u32,
}

impl PCFSServerState {
	#[must_use]
	pub const fn new(
		disable_real_removal: bool,
		host_filesystem: HostFilesystem,
		pid: u32,
	) -> Self {
		PCFSServerState {
			disable_real_removal,
			host_filesystem,
			pid,
		}
	}

	/// If we should actually disable real removal of files.
	#[must_use]
	pub const fn disable_real_removal(&self) -> bool {
		self.disable_real_removal
	}

	#[must_use]
	pub const fn host_filesystem(&self) -> &HostFilesystem {
		&self.host_filesystem
	}

	#[must_use]
	pub const fn pid(&self) -> u32 {
		self.pid
	}
}

impl FromRef<PCFSServerState> for HostFilesystem {
	fn from_ref(input: &PCFSServerState) -> Self {
		input.host_filesystem.clone()
	}
}

impl FromRef<PCFSServerState> for u32 {
	fn from_ref(input: &PCFSServerState) -> Self {
		input.pid
	}
}

/// Get a TCP server that is capable of serving PCFS-SATA to a cat-dev console.
///
/// ## Errors
///
/// If we cannot lookup the address to bind too, or there's been some
/// programming error, and we run into an API error calling an internal
/// api wrong.
#[allow(
	// TODO(mythra): we should probably extract this out into a builder
	// pattern some day. That day is not today.
	clippy::too_many_arguments,
	clippy::fn_params_excessive_bools,
)]
pub async fn pcfs_sata_server(
	host_filesystem: HostFilesystem,
	address: Option<Ipv4Addr>,
	port: Option<u16>,
	disable_ffio: bool,
	disable_csr: bool,
	disable_real_removal: bool,
	cat_dev_sleep_override: Option<Duration>,
	fully_disable_cat_dev_sleep: bool,
	chunk_override: Option<usize>,
	fully_disable_chunk_override: bool,
	trace_during_debug: bool,
) -> Result<TCPServer<PCFSServerState>, CatBridgeError> {
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
	let bound_address = SocketAddrV4::new(ip, port.unwrap_or(DEFAULT_SATA_PORT));

	let mut router = Router::<PCFSServerState>::new_with_offset(0x30);
	router.add_route(&0x0_u32.to_be_bytes(), create_folder::handle_create_folder)?;
	router.add_route(&0x1_u32.to_be_bytes(), open_folder::handle_open_folder)?;
	router.add_route(&0x2_u32.to_be_bytes(), read_folder::handle_read_folder)?;
	router.add_route(&0x3_u32.to_be_bytes(), rewind_folder::handle_rewind_folder)?;
	router.add_route(&0x4_u32.to_be_bytes(), close_folder::handle_close_folder)?;
	router.add_route(&0x5_u32.to_be_bytes(), open_file::handle_open_file)?;
	router.add_route(&0x6_u32.to_be_bytes(), read_file::handle_read_file)?;
	router.add_route(&0x7_u32.to_be_bytes(), write_file::handle_write_file)?;
	router.add_route(&0xB_u32.to_be_bytes(), info_by_query::stat_fd)?;
	router.add_route(&0xD_u32.to_be_bytes(), close_file::handle_close_file)?;
	router.add_route(&0xE_u32.to_be_bytes(), remove::handle_removal)?;
	router.add_route(
		&0x10_u32.to_be_bytes(),
		info_by_query::handle_get_info_by_query,
	)?;
	router.add_route(&0x12_u32.to_be_bytes(), change_owner::handle_change_owner)?;
	router.add_route(&0x13_u32.to_be_bytes(), change_mode::handle_change_mode)?;
	router.add_route(&0x14_u32.to_be_bytes(), ping::handle_ping)?;
	router.fallback_handler(unknown_packet_handler)?;

	let mut server = TCPServer::new_with_state(
		"pcfs-sata",
		bound_address,
		router,
		(None, None),
		NagleGuard::U32LengthPrefixed(Endianness::Big, Some(0x20)),
		PCFSServerState::new(disable_real_removal, host_filesystem, std::process::id()),
		trace_during_debug,
	)
	.await?;

	server.set_on_stream_begin(async move |event: ResponseStreamEvent<PCFSServerState>| {
		let sid = event.stream_id();

		_ = SATA_CONNECTION_FLAGS
			.insert_async(
				sid,
				SataConnectionFlags::new_with_flags(!disable_ffio, !disable_csr),
			)
			.await;

		Ok(true)
	})?;
	server.set_on_stream_end(on_sata_stream_end)?;
	server.layer_initial_service(
		ServiceBuilder::new()
			.layer(RequestIDLayer)
			.layer(StreamIDLayer)
			.layer(SataConnectionFlagsLayer),
	);

	server.set_chunk_output_at_size(if fully_disable_chunk_override {
		None
	} else if let Some(over_ride) = chunk_override {
		Some(over_ride)
	} else {
		Some(DEFAULT_CAT_DEV_CHUNK_SIZE)
	});
	server.set_cat_dev_slowdown(if fully_disable_cat_dev_sleep {
		None
	} else {
		Some(cat_dev_sleep_override.unwrap_or(DEFAULT_CAT_DEV_SLOWDOWN))
	});

	Ok(server)
}

async fn unknown_packet_handler(Body(request): Body<Bytes>) -> Response {
	if let Ok(req) = SataRequest::<Bytes>::parse_opaque(request) {
		warn!(
			header = valuable(req.header()),
			command_info = valuable(req.command_info()),
			body = format!("{:02X?}", req.body()),
			"Unknown PCFS Sata packet!",
		);
	}

	Response::empty_close()
}

/// Gets called when an PCFS SATA stream ends.
///
/// This is where we actually 'cleanup' all the data related to the PCFS
/// stream. For us this really just means clearing the stream id from the map
/// of version/capability flags we're using.
///
/// ## Errors
///
/// This shouldn't ever error, but needs to make the signature pass to auto
/// turn into a tower service.
async fn on_sata_stream_end(
	event: ResponseStreamEvent<PCFSServerState>,
) -> Result<(), CatBridgeError> {
	let sid = event.stream_id();
	_ = SATA_CONNECTION_FLAGS.remove_async(&sid).await;
	Ok(())
}
