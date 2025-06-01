//! Handles fetching state that is dynamic per request (e.g. a request id) that
//! can be attached.
//!
//! If you're dealing with app/server wide state, prefer [`crate::net::requestable::State`].

use crate::{
	errors::CatBridgeError,
	net::{
		errors::CommonNetAPIError,
		models::{FromRef, FromRequest, FromRequestParts, Request},
		server::models::{FromResponseStreamEvent, ResponseStreamEvent},
	},
};
use std::{
	fmt::{Debug, Formatter, Result as FmtResult},
	ops::{Deref, DerefMut},
	task::{Context, Poll},
};
use tower::{Layer, Service};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// A type that has been cloned, and extracted out of an extension map.
pub struct Extension<Ty>(pub Ty);

impl<Ty, State: Clone + Send + Sync + 'static> FromRef<Request<State>> for Option<Extension<Ty>>
where
	Ty: Clone + Send + Sync + 'static,
{
	fn from_ref(input: &Request<State>) -> Self {
		input
			.extensions()
			.get::<Ty>()
			.map(|ext| Extension(ext.clone()))
	}
}

impl<Ty, State: Clone + Send + Sync + 'static> FromRequestParts<State> for Extension<Ty>
where
	Ty: Clone + Send + Sync + 'static,
{
	async fn from_request_parts(req: &mut Request<State>) -> Result<Self, CatBridgeError> {
		req.extensions()
			.get::<Ty>()
			.map(|ext| Extension(ext.clone()))
			.ok_or_else(|| CommonNetAPIError::ExtensionNotPresent.into())
	}
}

impl<Ty, State: Clone + Send + Sync + 'static> FromRequest<State> for Extension<Ty>
where
	Ty: Clone + Send + Sync + 'static,
{
	async fn from_request(req: Request<State>) -> Result<Self, CatBridgeError> {
		req.extensions()
			.get::<Ty>()
			.map(|ext| Extension(ext.clone()))
			.ok_or_else(|| CommonNetAPIError::ExtensionNotPresent.into())
	}
}

impl<Ty, State: Clone + Send + Sync + 'static> FromResponseStreamEvent<State> for Extension<Ty>
where
	Ty: Clone + Send + Sync + 'static,
{
	async fn from_stream_event(
		event: &mut ResponseStreamEvent<State>,
	) -> Result<Self, CatBridgeError> {
		event
			.extensions()
			.get::<Ty>()
			.map(|ext| Extension(ext.clone()))
			.ok_or_else(|| CommonNetAPIError::ExtensionNotPresent.into())
	}
}

impl<ServiceTy, ExtensionTy> Layer<ServiceTy> for Extension<ExtensionTy>
where
	ExtensionTy: Clone + Send + Sync + 'static,
{
	type Service = AddExtension<ServiceTy, ExtensionTy>;

	fn layer(&self, inner: ServiceTy) -> Self::Service {
		AddExtension {
			value: self.0.clone(),
			wrapped: inner,
		}
	}
}

impl<Ty: Clone + Send + Sync + 'static> Deref for Extension<Ty> {
	type Target = Ty;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl<Ty: Clone + Send + Sync + 'static> DerefMut for Extension<Ty> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}

impl<Ty> Debug for Extension<Ty>
where
	Ty: Debug,
{
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		fmt.debug_struct("Extension")
			.field("inner", &self.0)
			.finish()
	}
}

const EXTENSION_FIELDS: &[NamedField<'static>] = &[NamedField::new("inner")];

impl<Ty> Structable for Extension<Ty>
where
	Ty: Valuable,
{
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static("Extension", Fields::Named(EXTENSION_FIELDS))
	}
}

impl<Ty> Valuable for Extension<Ty>
where
	Ty: Valuable,
{
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(EXTENSION_FIELDS, &[self.0.as_value()]));
	}
}

/// Middleware for adding some shareable value to a request extensions field.
///
/// This is the actual tower service that gets processed during requests.
#[derive(Clone)]
pub struct AddExtension<ServiceTy, ExtensionTy> {
	value: ExtensionTy,
	wrapped: ServiceTy,
}

impl<ServiceTy, ExtensionTy, State> Service<Request<State>> for AddExtension<ServiceTy, ExtensionTy>
where
	ServiceTy: Service<Request<State>>,
	ExtensionTy: Clone + Send + Sync + 'static,
	State: Clone + Send + Sync + 'static,
{
	type Response = ServiceTy::Response;
	type Error = ServiceTy::Error;
	type Future = ServiceTy::Future;

	#[inline]
	fn poll_ready(&mut self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.wrapped.poll_ready(ctx)
	}

	fn call(&mut self, mut req: Request<State>) -> Self::Future {
		req.extensions_mut().insert(self.value.clone());
		self.wrapped.call(req)
	}
}

impl<ServiceTy, ExtensionTy, State> Service<ResponseStreamEvent<State>>
	for AddExtension<ServiceTy, ExtensionTy>
where
	ServiceTy: Service<ResponseStreamEvent<State>>,
	ExtensionTy: Clone + Send + Sync + 'static,
	State: Clone + Send + Sync + 'static,
{
	type Response = ServiceTy::Response;
	type Error = ServiceTy::Error;
	type Future = ServiceTy::Future;

	#[inline]
	fn poll_ready(&mut self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.wrapped.poll_ready(ctx)
	}

	fn call(&mut self, mut evt: ResponseStreamEvent<State>) -> Self::Future {
		evt.extensions_mut().insert(self.value.clone());
		self.wrapped.call(evt)
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use crate::net::server::{Router, test_helpers::router_body_no_close};
	use std::sync::{Arc, atomic::AtomicU8};

	#[tokio::test]
	pub async fn test_extension() {
		async fn echo_extension(
			Extension(data): Extension<String>,
			Extension(_unused): Extension<Arc<AtomicU8>>,
		) -> String {
			data
		}

		let mut router = Router::new();
		router
			.add_route(&[0x1], echo_extension)
			.expect("Failed to add route to router!");
		router.layer(Extension::<String>("Hey from an extension!".to_owned()));
		router.layer(Extension::<Arc<AtomicU8>>(Arc::new(AtomicU8::new(0))));
		assert_eq!(
			router_body_no_close(&mut router, &[0x1, 0x2, 0x3, 0x4]).await,
			b"Hey from an extension!",
		);
	}
}
