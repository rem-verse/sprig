//! A unique identifier for a request.
//!
//! Request ID's are not guaranteed to be unique, and should not be used as a
//! globally unique identifier. Request ID's only guarantee that you have an
//! incredibly high likelyhood of having a globally unique identifier during
//! the requests lifespan only.
//!
//! Their main purpose is to scope debugging from ALL REQUESTS to a very narrow
//! scope, and hopefully to a single request especially during the same period.

use crate::{
	errors::CatBridgeError,
	net::{
		errors::CommonNetAPIError,
		models::{FromRequest, FromRequestParts, Request, Response},
	},
};
use rand::{TryRngCore, rng};
use std::{
	convert::Infallible,
	fmt::{Debug, Display, Formatter, Result as FmtResult, Write},
	ops::Deref,
	sync::Arc,
};
use tower::{Layer, Service};
use tracing::{
	Id as TracingId, error_span,
	field::valuable,
	instrument::{Instrument, Instrumented},
};
use valuable::{Valuable, Value, Visit};

#[derive(Clone, PartialEq, Eq)]
pub struct RequestID(Arc<String>);

impl RequestID {
	/// Generate a brand new request-id that is probably "unique".
	///
	/// ... at least it is as long as you wish upon a star.
	#[must_use]
	pub fn generate() -> Self {
		let mut buff = [0_u8; 16];
		// If we don't feed in randomness here this is fine. Request ID's don't
		// need to be unique _techincally_.
		_ = rng().try_fill_bytes(&mut buff);
		let mut id = String::with_capacity(32);
		for byte in buff {
			_ = write!(&mut id, "{byte:02x}");
		}
		Self(Arc::new(id))
	}

	/// Create a request-id from an already existing string.
	///
	/// This allows for creating shared request-id's across services.
	#[must_use]
	pub fn from_existing(id: String) -> Self {
		Self(Arc::new(id))
	}

	/// Create a request-id pointer to an unknown request-id.
	///
	/// Use this when you expect one to be set, but couldn't find one in a fatal
	/// error condition.
	#[must_use]
	pub fn fatal_unknown() -> Self {
		Self(Arc::new("<unknown>".to_owned()))
	}

	/// Get a reference to this request-id as a string.
	#[must_use]
	pub fn str(&self) -> &str {
		self.0.as_str()
	}
}

impl<State: Clone + Send + Sync + 'static> FromRequestParts<State> for RequestID {
	async fn from_request_parts(parts: &mut Request<State>) -> Result<Self, CatBridgeError> {
		parts
			.extensions()
			.get::<RequestID>()
			.cloned()
			.ok_or(CommonNetAPIError::ExtensionNotPresent.into())
	}
}

impl<State: Clone + Send + Sync + 'static> FromRequest<State> for RequestID {
	async fn from_request(req: Request<State>) -> Result<Self, CatBridgeError> {
		req.extensions()
			.get::<RequestID>()
			.cloned()
			.ok_or(CommonNetAPIError::ExtensionNotPresent.into())
	}
}

impl Debug for RequestID {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		fmt.debug_struct("RequestID").field("id", &self.0).finish()
	}
}

impl Display for RequestID {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		write!(fmt, "{}", self.0)
	}
}

impl Deref for RequestID {
	type Target = str;

	fn deref(&self) -> &Self::Target {
		self.str()
	}
}

impl Valuable for RequestID {
	fn as_value(&self) -> Value<'_> {
		Value::String(self.0.as_str())
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_value(self.as_value());
	}
}

#[derive(Clone, Debug)]
pub struct RequestIDLayer(String);

impl RequestIDLayer {
	#[must_use]
	pub const fn new(service_name: String) -> Self {
		Self(service_name)
	}
}

impl<Layered> Layer<Layered> for RequestIDLayer
where
	Layered: Clone,
{
	type Service = LayeredRequestID<Layered>;

	fn layer(&self, inner: Layered) -> Self::Service {
		LayeredRequestID {
			inner,
			service_name: self.0.clone(),
		}
	}
}

#[derive(Clone)]
pub struct LayeredRequestID<Layered> {
	inner: Layered,
	service_name: String,
}

impl<Layered, State: Clone + Send + Sync + 'static> Service<Request<State>>
	for LayeredRequestID<Layered>
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
		let req_id = RequestID::generate();

		let span = error_span!(
			parent: parent_span,
			"WithRequestID",
			lisa.subsystem = %self.service_name,
			request.id = valuable(&req_id),
		);
		req.extensions_mut().insert::<RequestID>(req_id);
		req.extensions_mut().insert::<Option<TracingId>>(span.id());
		self.inner.call(req).instrument(span.or_current())
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	fn only_accept<Ty: Clone + Send + Sync + 'static>(_unused: Option<Ty>) {}

	#[test]
	pub fn assert_is_extensionable() {
		only_accept::<RequestID>(None);
	}
}
