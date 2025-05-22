//! A layer that will parse out header information from a particular packet.
//!
//! This is the layer that automatically parses out the `SataCommandInfo`, and
//! `SataHeader` fields for packets, meaning body deserialization just has to
//! worry about well.... the body.

use crate::{
	errors::CatBridgeError,
	fsemul::pcfs::{
		errors::PCFSApiError,
		sata::proto::{SataCommandInfo, SataPacketHeader, SataRequest},
	},
	net::models::{FromRequest, FromRequestParts, Request, Response},
};
use std::{
	convert::Infallible,
	pin::Pin,
	task::{Context, Poll},
};
use tower::{Layer, Service};
use tracing::warn;

#[derive(Clone, Debug)]
pub struct SataHeaderParserLayer;

impl<Layered> Layer<Layered> for SataHeaderParserLayer
where
	Layered: Clone,
{
	type Service = LayeredSataHeaderParser<Layered>;

	fn layer(&self, inner: Layered) -> Self::Service {
		LayeredSataHeaderParser { inner }
	}
}

#[derive(Clone)]
pub struct LayeredSataHeaderParser<Layered> {
	inner: Layered,
}

impl<Layered, State: Clone + Send + Sync + 'static> Service<Request<State>>
	for LayeredSataHeaderParser<Layered>
where
	Layered:
		Service<Request<State>, Response = Response, Error = Infallible> + Clone + Send + 'static,
	Layered::Future: Send + 'static,
{
	type Response = Layered::Response;
	type Error = Layered::Error;
	type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

	#[inline]
	fn poll_ready(&mut self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.inner.poll_ready(ctx)
	}

	fn call(&mut self, mut req: Request<State>) -> Self::Future {
		let mut inner_clone = self.inner.clone();

		Box::pin(async move {
			let mut body = req.body().clone();
			let (as_header, ci, real_body) = match SataRequest::parse_opaque(body) {
				Ok(success) => success.into_parts(),
				Err(cause) => {
					warn!(
						?cause,
						"sata command info parse failed, will close connection"
					);
					return Ok::<Response, Infallible>(Response::empty_close());
				}
			};

			req.extensions_mut().insert(as_header);
			req.extensions_mut().insert(ci);
			req.swap_body(real_body);

			match inner_clone.call(req).await {
				Ok(resp) => Ok::<Response, Infallible>(resp),
				Err(cause) => {
					warn!(?cause, "sata packet handler failed, will close connection");
					Ok::<Response, Infallible>(Response::empty_close())
				}
			}
		})
	}
}

impl<State: Clone + Send + Sync + 'static> FromRequestParts<State> for SataCommandInfo {
	async fn from_request_parts(req: &mut Request<State>) -> Result<Self, CatBridgeError> {
		req.extensions().get::<SataCommandInfo>().ok_or_else(|| {
			PCFSApiError::MissingCriticalExtension("SataCommandInfo".to_owned()).into()
		})
	}
}
impl<State: Clone + Send + Sync + 'static> FromRequest<State> for SataCommandInfo {
	async fn from_request(req: Request<State>) -> Result<Self, CatBridgeError> {
		req.extensions_owned()
			.remove::<SataCommandInfo>()
			.ok_or_else(|| {
				PCFSApiError::MissingCriticalExtension("SataCommandInfo".to_owned()).into()
			})
	}
}

impl<State: Clone + Send + Sync + 'static> FromRequestParts<State> for SataPacketHeader {
	async fn from_request_parts(req: &mut Request<State>) -> Result<Self, CatBridgeError> {
		req.extensions().get::<SataPacketHeader>().ok_or_else(|| {
			PCFSApiError::MissingCriticalExtension("SataPacketHeader".to_owned()).into()
		})
	}
}
impl<State: Clone + Send + Sync + 'static> FromRequest<State> for SataPacketHeader {
	async fn from_request(req: Request<State>) -> Result<Self, CatBridgeError> {
		req.extensions_owned()
			.remove::<SataPacketHeader>()
			.ok_or_else(|| {
				PCFSApiError::MissingCriticalExtension("SataPacketHeader".to_owned()).into()
			})
	}
}
