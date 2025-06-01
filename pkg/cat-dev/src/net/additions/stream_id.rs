//! A unique identifier for a specific 'live' connection, or TCP Stream.
//!
//! On UDP this will still have a pretty strong guarantee of being unique.
//! Still a lot less guarantees can be made, but the hope it is still decently
//! unique.

use crate::{
	errors::CatBridgeError,
	net::models::{FromRequest, FromRequestParts, Request, Response},
};
use std::{
	convert::Infallible,
	fmt::{Display, Formatter, Result as FmtResult},
	ops::Deref,
};
use tower::{Layer, Service};
use tracing::{
	Id as TracingId, error_span,
	instrument::{Instrument, Instrumented},
};
use valuable::Valuable;

/// A unique identifier for a particular TCP Stream, or connection.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Valuable)]
pub struct StreamID(u64);

impl StreamID {
	#[must_use]
	pub const fn from_existing(id: u64) -> Self {
		Self(id)
	}

	/// Convert this stream id to a raw value.
	#[must_use]
	pub fn to_raw(&self) -> u64 {
		self.0
	}
}

impl Display for StreamID {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		write!(fmt, "{}", self.0)
	}
}

impl Deref for StreamID {
	type Target = u64;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl<State: Clone + Send + Sync + 'static> FromRequestParts<State> for StreamID {
	async fn from_request_parts(parts: &mut Request<State>) -> Result<Self, CatBridgeError> {
		Ok(Self::from_existing(parts.stream_id()))
	}
}

impl<State: Clone + Send + Sync + 'static> FromRequest<State> for StreamID {
	async fn from_request(req: Request<State>) -> Result<Self, CatBridgeError> {
		Ok(Self::from_existing(req.stream_id()))
	}
}

#[derive(Clone, Debug)]
pub struct StreamIDLayer;

impl<Layered> Layer<Layered> for StreamIDLayer
where
	Layered: Clone,
{
	type Service = LayeredStreamID<Layered>;

	fn layer(&self, inner: Layered) -> Self::Service {
		LayeredStreamID { inner }
	}
}

#[derive(Clone)]
pub struct LayeredStreamID<Layered> {
	inner: Layered,
}

impl<Layered, State: Clone + Send + Sync + 'static> Service<Request<State>>
	for LayeredStreamID<Layered>
where
	Layered:
		Service<Request<State>, Response = Response, Error = Infallible> + Clone + Send + 'static,
	Layered::Future: Send + 'static,
{
	type Response = Layered::Response;
	type Error = Layered::Error;
	type Future = Instrumented<Layered::Future>;

	#[inline]
	fn poll_ready(
		&mut self,
		ctx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Result<(), Self::Error>> {
		self.inner.poll_ready(ctx)
	}

	fn call(&mut self, mut req: Request<State>) -> Self::Future {
		let parent_span = req
			.extensions()
			.get::<Option<TracingId>>()
			.cloned()
			.unwrap_or(None);
		let stream_id = StreamID::from_existing(req.stream_id());

		let span = error_span!(
		  parent: parent_span,
		  "WithStreamID",
		  request.stream_id = %stream_id,
		);
		req.extensions_mut().insert::<StreamID>(stream_id);
		req.extensions_mut().insert::<Option<TracingId>>(span.id());
		self.inner.call(req).instrument(span.or_current())
	}
}
