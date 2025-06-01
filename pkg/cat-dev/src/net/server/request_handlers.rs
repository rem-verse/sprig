use crate::{
	errors::CatBridgeError,
	net::models::{FromRequest, FromRequestParts, IntoResponse, Request, Response},
};
use std::{
	marker::PhantomData,
	pin::Pin,
	task::{Context, Poll},
};
use tower::Service;

/// A route handler, attempts to be an incredibly thin layer between a
/// function, and the actual ending handler.
///
/// Implemented so we can implement for function that accepts any varying
/// amount of args of types that implement from request parts.
///
/// `ParamTy` is kept to prevent generation of conflicting type implementations
/// of this trait. It however is not actually needed by any of our code.
pub trait Handler<ParamTy, State: Clone + Send + Sync + 'static> {
	type Future: Future<Output = Result<Response, CatBridgeError>> + Send + 'static;

	fn call(self, req: Request<State>) -> Self::Future;
}

/// Allow any async function without arguments to be a handler
impl<UnderlyingFnType, FnFutureTy, ResponseTy, State: Clone + Send + Sync + 'static>
	Handler<(), State> for UnderlyingFnType
where
	UnderlyingFnType: FnOnce() -> FnFutureTy + Clone + Send + 'static,
	FnFutureTy: Future<Output = ResponseTy> + Send,
	ResponseTy: IntoResponse,
{
	type Future = Pin<Box<dyn Future<Output = Result<Response, CatBridgeError>> + Send>>;

	fn call(self, _req: Request<State>) -> Self::Future {
		Box::pin(async move { self().await.to_response() })
	}
}

macro_rules! fn_to_handler {
	(
		[$($ty:ident),*], $last:ident
	) => {
		#[allow(non_snake_case, unused_mut)]
		impl<UnderlyingFnType, FnFutureTy, OutputTy, State: Clone + Send + Sync + 'static, $($ty,)* $last> Handler<($($ty,)* $last,), State> for UnderlyingFnType
		where
			UnderlyingFnType: FnOnce($($ty,)* $last) -> FnFutureTy + Clone + Send + 'static,
			FnFutureTy: Future<Output = OutputTy> + Send,
			OutputTy: IntoResponse,
			$( $ty: FromRequestParts<State> + Send, )*
			$last: FromRequest<State> + Send,
		{
			type Future = Pin<Box<dyn Future<Output = Result<Response, CatBridgeError>> + Send>>;

			fn call(self, mut req: Request<State>) -> Self::Future {
				Box::pin(async move {
					$(
						let $ty = $ty::from_request_parts(&mut req).await?;
					)*
					let $last = $last::from_request(req).await?;
					let res = self($($ty,)* $last).await;
					res.to_response()
				})
			}
		}
	}
}

fn_to_handler!([], T1);
fn_to_handler!([T1], T2);
fn_to_handler!([T1, T2], T3);
fn_to_handler!([T1, T2, T3], T4);
fn_to_handler!([T1, T2, T3, T4], T5);
fn_to_handler!([T1, T2, T3, T4, T5], T6);
fn_to_handler!([T1, T2, T3, T4, T5, T6], T7);
fn_to_handler!([T1, T2, T3, T4, T5, T6, T7], T8);
fn_to_handler!([T1, T2, T3, T4, T5, T6, T7, T8], T9);
fn_to_handler!([T1, T2, T3, T4, T5, T6, T7, T8, T9], T10);
fn_to_handler!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10], T11);
fn_to_handler!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11], T12);
fn_to_handler!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12], T13);
fn_to_handler!(
	[T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13],
	T14
);
fn_to_handler!(
	[T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14],
	T15
);
fn_to_handler!(
	[
		T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15
	],
	T16
);

/// A wrapper around a handler to implement a service.
pub struct HandlerAsService<HandlerTy, HandlerParamsTy> {
	handler: HandlerTy,
	marker: PhantomData<HandlerParamsTy>,
}

impl<HandlerTy, HandlerParamsTy> HandlerAsService<HandlerTy, HandlerParamsTy> {
	#[must_use]
	pub(crate) fn new(handler: HandlerTy) -> Self {
		Self {
			handler,
			marker: PhantomData,
		}
	}
}

impl<HandlerTy, HandlerParamsTy> Clone for HandlerAsService<HandlerTy, HandlerParamsTy>
where
	HandlerTy: Clone,
{
	fn clone(&self) -> Self {
		Self {
			handler: self.handler.clone(),
			marker: PhantomData,
		}
	}
}

impl<HandlerTy, HandlerParamsTy, State: Clone + Send + Sync + 'static> Service<Request<State>>
	for HandlerAsService<HandlerTy, HandlerParamsTy>
where
	HandlerTy: Handler<HandlerParamsTy, State> + Clone + Send + 'static,
{
	type Response = Response;
	type Error = CatBridgeError;
	type Future = HandlerTy::Future;

	#[inline]
	fn poll_ready(&mut self, _ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		Poll::Ready(Ok(()))
	}

	fn call(&mut self, req: Request<State>) -> Self::Future {
		let handler = self.handler.clone();
		handler.call(req)
	}
}
