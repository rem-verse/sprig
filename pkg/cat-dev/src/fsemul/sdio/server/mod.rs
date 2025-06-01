//! "Server" implementations for SDIO protocols for handling PCFS for cat-dev.
//!
//! Reminder that SDIO Server, actually _connects_ to an open TCP port (like a
//! client normally would). It is called SDIO Server as it still
//! processes packets as a server.

mod message;
mod read;

use crate::{
	errors::CatBridgeError,
	fsemul::{
		HostFilesystem,
		sdio::{
			DEFAULT_SDIO_BLOCK_PORT, DEFAULT_SDIO_CONTROL_PORT, SDIO_DATA_STREAMS,
			data_stream::DataStream, proto::SdioControlPacketType,
		},
	},
	net::{
		DEFAULT_CAT_DEV_CHUNK_SIZE, DEFAULT_CAT_DEV_SLOWDOWN,
		additions::{RequestIDLayer, StreamIDLayer},
		models::FromRef,
		server::{Router, TCPServer, models::ResponseStreamEvent},
	},
};
use scc::HashMap as ConcurrentHashMap;
use std::{
	net::{Ipv4Addr, SocketAddr, SocketAddrV4},
	sync::LazyLock,
	time::Duration,
};
use tower::ServiceBuilder;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// Underlying 'printf' buffers used to buffer messages that are incomplete.
///
/// The SDIO stream also supports a serial style logging. However, again as a
/// TCP Stream packets may be split even across the same `write` call. This
/// buffer ensures we only flush when we have a complete line.
///
/// Similar to SDIO Data Streams, this is created on SDIO Control stream start,
/// and gets claned up on SDIO Control stream end.
static SDIO_PRINTF_BUFFS: LazyLock<ConcurrentHashMap<u64, String>> =
	LazyLock::new(|| ConcurrentHashMap::with_capacity(1));

/// Get a TCP server that is capable of serving SDIO to a cat-dev console.
///
/// Unlike most "TCP Servers" though this is actually created by connecting to
/// the console directly. We don't actually bind a port on the host-pc side
/// that listens that the console connects to. Instead we're a server that
/// _Connects_ to our client.
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
)]
pub async fn sdio_server(
	mion_ip: Ipv4Addr,
	control_port: Option<u16>,
	data_port: Option<u16>,
	host_filesystem: &HostFilesystem,
	cat_dev_sleep_override: Option<Duration>,
	fully_disable_cat_dev_sleep: bool,
	chunk_override: Option<usize>,
	fully_disable_chunk_override: bool,
	trace_during_debug: bool,
) -> Result<TCPServer<SDIOStreamState>, CatBridgeError> {
	let control_port = control_port.unwrap_or(DEFAULT_SDIO_CONTROL_PORT);
	let data_port = data_port.unwrap_or(DEFAULT_SDIO_BLOCK_PORT);
	let chunk_frd = if fully_disable_chunk_override {
		None
	} else if let Some(over_ride) = chunk_override {
		Some(over_ride)
	} else {
		Some(DEFAULT_CAT_DEV_CHUNK_SIZE)
	};

	let mut router = Router::<SDIOStreamState>::new();
	router.add_route(
		&[u8::from(SdioControlPacketType::Read)],
		read::handle_read_request,
	)?;
	router.add_route(
		&[u8::from(SdioControlPacketType::Message)],
		message::handle_message,
	)?;

	let mut control_server = TCPServer::new_with_state(
		"sdio",
		SocketAddr::V4(SocketAddrV4::new(mion_ip, control_port)),
		router,
		(None, None),
		512_usize,
		SDIOStreamState::new(
			chunk_frd,
			data_port,
			host_filesystem.clone(),
			if fully_disable_cat_dev_sleep {
				None
			} else {
				Some(cat_dev_sleep_override.unwrap_or(DEFAULT_CAT_DEV_SLOWDOWN))
			},
			#[cfg(debug_assertions)]
			trace_during_debug,
		),
		trace_during_debug,
	)
	.await?;
	control_server.set_on_stream_begin(on_sdio_stream_begin)?;
	control_server.set_on_stream_end(on_sdio_stream_end)?;
	control_server.layer_initial_service(
		ServiceBuilder::new()
			.layer(RequestIDLayer)
			.layer(StreamIDLayer),
	);
	// We are still communicating with a slowdown...
	control_server.set_cat_dev_slowdown(control_server.state().cat_dev_slowdown);
	control_server.set_chunk_output_at_size(chunk_frd);

	Ok(control_server)
}

/// Gets called when an SDIO Control stream begins.
///
/// This is where we actually connect to data-streams, and initialize our
/// PRINTF buffs. Right now this is fairly 'overkill' as TCP Servers using
/// the connection model (which SDIO uses) only actually have one underlying
/// stream, and can't possibly have multiple.
///
/// This means multiple SDIO servers will have to be created in order to
/// balance multiple connections. This mirrors the official SDK behavior.
///
/// ## Errors
///
/// If we cannot connect to the other SDIO data stream.
async fn on_sdio_stream_begin(
	event: ResponseStreamEvent<SDIOStreamState>,
) -> Result<bool, CatBridgeError> {
	let mut addr = *event.source();
	addr.set_port(event.state().data_port);
	let stream = DataStream::connect(
		addr,
		event.state().cat_dev_slowdown,
		event.state().chunk_size,
		#[cfg(debug_assertions)]
		event.state().trace_during_debug,
	)
	.await?;
	let sid = event.stream_id();

	_ = SDIO_DATA_STREAMS.insert_async(sid, stream).await;
	_ = SDIO_PRINTF_BUFFS
		.insert_async(sid, String::with_capacity(0))
		.await;

	Ok(true)
}

/// Gets called when an SDIO Control stream ends.
///
/// This is where we actually 'cleanup' all the data related to the SDIO data
/// stream. This includes both the printf buff, but also dropping the
/// tcp stream.
///
/// ## Errors
///
/// This shouldn't ever error, but needs to make the signature pass to auto
/// turn into a tower service.
async fn on_sdio_stream_end(
	event: ResponseStreamEvent<SDIOStreamState>,
) -> Result<(), CatBridgeError> {
	let sid = event.stream_id();

	_ = SDIO_DATA_STREAMS.remove_async(&sid).await;
	_ = SDIO_PRINTF_BUFFS.remove_async(&sid).await;

	Ok(())
}

/// The state of the SDIO Control server that every client ends up interacting
/// with.
///
/// Most folks will only actually use the host filesystem from this state, and
/// not interact with it directly.
#[derive(Clone, Debug)]
pub struct SDIOStreamState {
	/// The slowdown to apply to data stream connections.
	cat_dev_slowdown: Option<Duration>,
	/// How much we should chunk packets coming out of the data stream.
	chunk_size: Option<usize>,
	/// The data port to connect to data streams on.
	data_port: u16,
	/// The host filesystem that actually contains pointers to the filesystem.
	host_fs: HostFilesystem,
	/// Trace when debug mode is active.
	#[cfg(debug_assertions)]
	trace_during_debug: bool,
}

impl SDIOStreamState {
	#[must_use]
	pub fn new(
		chunk_size: Option<usize>,
		data_port: u16,
		host_fs: HostFilesystem,
		cat_dev_sleep: Option<Duration>,
		#[cfg(debug_assertions)]
		trace_during_debug: bool,
	) -> Self {
		Self {
			chunk_size,
			cat_dev_slowdown: cat_dev_sleep,
			data_port,
			host_fs,
			#[cfg(debug_assertions)]
			trace_during_debug,
		}
	}
}

impl FromRef<SDIOStreamState> for HostFilesystem {
	fn from_ref(input: &SDIOStreamState) -> Self {
		input.host_fs.clone()
	}
}

const SDIO_STREAM_STATE_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("cat_dev_slowdown"),
	NamedField::new("data_port"),
	NamedField::new("host_fs"),
];

impl Structable for SDIOStreamState {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static("SDIOStreamState", Fields::Named(SDIO_STREAM_STATE_FIELDS))
	}
}

impl Valuable for SDIOStreamState {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SDIO_STREAM_STATE_FIELDS,
			&[
				Valuable::as_value(&if let Some(slowdown) = self.cat_dev_slowdown {
					slowdown.as_secs()
				} else {
					0_u64
				}),
				Valuable::as_value(&self.data_port),
				Valuable::as_value(&self.host_fs),
			],
		));
	}
}
