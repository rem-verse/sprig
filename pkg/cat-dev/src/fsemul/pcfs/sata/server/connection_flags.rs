//! Implement connection level flags for each connection that is present.
//!
//! The known connection flags, or things that can change/be affected by a
//! whole connection at once with PCFS are:
//!
//! 1. "Fast-File I/O Enabled", whether or not we should be using the 'ffio'
//!    version of PCFS over SATA, where file data can be inline on the stream
//!    without needing to be in a series of chunked packets.
//! 2. "Combined Send/Recv Enabled", whether or not we should support using
//!    'csr' in PCFS over sata, where sends/receives can be bundled together.
//! 3. "Version": the version of PCFS that we should report to be for this
//!    client.

use crate::{
	errors::CatBridgeError,
	fsemul::pcfs::sata::proto::DEFAULT_PCFS_VERSION,
	net::models::{FromRequest, FromRequestParts, Request, Response},
};
use scc::HashMap as ConcurrentHashMap;
use std::{
	convert::Infallible,
	sync::{
		Arc, LazyLock,
		atomic::{AtomicBool, AtomicU32, Ordering},
	},
	task::{Context, Poll},
};
use tower::{Layer, Service};
use valuable::Valuable;

/// A series of connection flags that apply to a connnection.
///
/// See this modules documentation for more information about more of what
/// these flags indicate, and what flags can be set.
pub(super) static SATA_CONNECTION_FLAGS: LazyLock<ConcurrentHashMap<u64, SataConnectionFlags>> =
	LazyLock::new(|| ConcurrentHashMap::with_capacity(1));

/// The flags that are set on this particular connection.
#[derive(Clone, Debug, Valuable)]
pub struct SataConnectionFlags {
	fast_file_io_enabled: Arc<AtomicBool>,
	combined_send_recv_enabled: Arc<AtomicBool>,
	version: Arc<AtomicU32>,
	first_read_size: Arc<AtomicU32>,
	first_write_size: Arc<AtomicU32>,
}

impl SataConnectionFlags {
	#[must_use]
	pub fn new() -> Self {
		Self {
			fast_file_io_enabled: Arc::new(AtomicBool::new(true)),
			combined_send_recv_enabled: Arc::new(AtomicBool::new(true)),
			version: Arc::new(AtomicU32::new(DEFAULT_PCFS_VERSION)),
			first_read_size: Arc::new(AtomicU32::new(196_672)),
			first_write_size: Arc::new(AtomicU32::new(196_640)),
		}
	}

	#[must_use]
	pub fn new_with_flags(ffio_enabled: bool, csr_enabled: bool) -> Self {
		Self {
			fast_file_io_enabled: Arc::new(AtomicBool::new(ffio_enabled)),
			combined_send_recv_enabled: Arc::new(AtomicBool::new(csr_enabled)),
			version: Arc::new(AtomicU32::new(DEFAULT_PCFS_VERSION)),
			first_read_size: Arc::new(AtomicU32::new(196_672)),
			first_write_size: Arc::new(AtomicU32::new(196_640)),
		}
	}

	#[must_use]
	pub fn ffio_enabled(&self) -> bool {
		self.fast_file_io_enabled.load(Ordering::Acquire)
	}

	pub fn set_ffio_enabled(&self, enabled: bool) {
		self.fast_file_io_enabled.store(enabled, Ordering::Release);
	}

	#[must_use]
	pub fn csr_enabled(&self) -> bool {
		self.combined_send_recv_enabled.load(Ordering::Acquire)
	}

	pub fn set_csr_enabled(&self, enabled: bool) {
		self.combined_send_recv_enabled
			.store(enabled, Ordering::Release);
	}

	#[must_use]
	pub fn version(&self) -> u32 {
		self.version.load(Ordering::Acquire)
	}

	pub fn set_version(&self, version_num: u32) {
		self.version.store(version_num, Ordering::Release);
	}

	#[must_use]
	pub fn first_read_size(&self) -> u32 {
		self.first_read_size.load(Ordering::Acquire)
	}

	pub fn set_first_read_size(&self, new_size: u32) {
		self.first_read_size.store(new_size, Ordering::Release);
	}

	#[must_use]
	pub fn first_write_size(&self) -> u32 {
		self.first_write_size.load(Ordering::Acquire)
	}

	pub fn set_first_write_size(&self, new_size: u32) {
		self.first_write_size.store(new_size, Ordering::Release);
	}
}

impl Default for SataConnectionFlags {
	fn default() -> Self {
		Self::new()
	}
}

#[derive(Clone, Debug)]
pub struct SataConnectionFlagsLayer;

impl<Layered> Layer<Layered> for SataConnectionFlagsLayer
where
	Layered: Clone,
{
	type Service = LayeredSataConnectionFlags<Layered>;

	fn layer(&self, inner: Layered) -> Self::Service {
		LayeredSataConnectionFlags { inner }
	}
}

#[derive(Clone)]
pub struct LayeredSataConnectionFlags<Layered> {
	inner: Layered,
}

impl<Layered, State: Clone + Send + Sync + 'static> Service<Request<State>>
	for LayeredSataConnectionFlags<Layered>
where
	Layered:
		Service<Request<State>, Response = Response, Error = Infallible> + Clone + Send + 'static,
	Layered::Future: Send + 'static,
{
	type Response = Layered::Response;
	type Error = Layered::Error;
	type Future = Layered::Future;

	#[inline]
	fn poll_ready(&mut self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.inner.poll_ready(ctx)
	}

	fn call(&mut self, mut req: Request<State>) -> Self::Future {
		if let Some(flags) = SATA_CONNECTION_FLAGS.get(&req.stream_id()) {
			req.extensions_mut().insert(flags.clone());
		}

		self.inner.call(req)
	}
}

impl<State: Clone + Send + Sync + 'static> FromRequestParts<State> for SataConnectionFlags {
	async fn from_request_parts(req: &mut Request<State>) -> Result<Self, CatBridgeError> {
		Ok(req
			.extensions()
			.get::<SataConnectionFlags>()
			.cloned()
			.unwrap_or_default())
	}
}

impl<State: Clone + Send + Sync + 'static> FromRequest<State> for SataConnectionFlags {
	async fn from_request(req: Request<State>) -> Result<Self, CatBridgeError> {
		Ok(req
			.extensions()
			.get::<SataConnectionFlags>()
			.cloned()
			.unwrap_or_default())
	}
}
