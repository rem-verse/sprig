//! Utilities to turn functions into "on stream begin"/"on connection"
//! handlers.

use crate::errors::CatBridgeError;
use std::{
	marker::PhantomData,
	pin::Pin,
	task::{Context, Poll},
};
use tower::Service;

#[cfg(feature = "clients")]
use crate::net::client::models::{FromRequestStreamEvent, RequestStreamEvent};
#[cfg(feature = "servers")]
use crate::net::server::models::{FromResponseStreamEvent, ResponseStreamEvent};

/// A handler for when a stream begins (an "on connection" event), attempts
/// to be an incredibly thin layer between a function, and the actual
/// ending handler.
///
/// Implemented so we can implement for function that accepts any varying
/// amount of args of types that implement from request parts.
///
/// `ParamTy` is kept to prevent generation of conflicting type implementations
/// of this trait. It however is not actually needed by any of our code.
#[cfg(feature = "clients")]
pub trait OnRequestStreamBeginHandler<ParamTy, State: Clone + Send + Sync + 'static> {
	type Future: Future<Output = Result<bool, CatBridgeError>> + Send + 'static;

	fn call(self, event: RequestStreamEvent<State>) -> Self::Future;
}

/// Allow any async function without arguments to be a handler
#[cfg(feature = "clients")]
impl<UnderlyingFnType, FnFutureTy, ResponseTy, State> OnRequestStreamBeginHandler<(), State>
	for UnderlyingFnType
where
	UnderlyingFnType: FnOnce() -> FnFutureTy + Clone + Send + 'static,
	FnFutureTy: Future<Output = ResponseTy> + Send,
	ResponseTy: Into<Result<bool, CatBridgeError>>,
	State: Clone + Send + Sync + 'static,
{
	type Future = Pin<Box<dyn Future<Output = Result<bool, CatBridgeError>> + Send>>;

	fn call(self, _event: RequestStreamEvent<State>) -> Self::Future {
		Box::pin(async move { self().await.into() })
	}
}

/// Allow any async function with a single consuming argument.
#[cfg(feature = "clients")]
impl<UnderlyingFnType, FnFutureTy, ResponseTy, State, ArgTy>
	OnRequestStreamBeginHandler<ArgTy, State> for UnderlyingFnType
where
	UnderlyingFnType: FnOnce(ArgTy) -> FnFutureTy + Clone + Send + 'static,
	FnFutureTy: Future<Output = ResponseTy> + Send,
	ResponseTy: Into<Result<bool, CatBridgeError>>,
	ArgTy: From<RequestStreamEvent<State>> + Send,
	State: Clone + Send + Sync + 'static,
{
	type Future = Pin<Box<dyn Future<Output = Result<bool, CatBridgeError>> + Send>>;

	fn call(self, event: RequestStreamEvent<State>) -> Self::Future {
		Box::pin(async move { self(ArgTy::from(event)).await.into() })
	}
}

/// A handler for when a stream begins (an "on connection" event), attempts
/// to be an incredibly thin layer between a function, and the actual
/// ending handler.
///
/// Implemented so we can implement for function that accepts any varying
/// amount of args of types that implement from request parts.
///
/// `ParamTy` is kept to prevent generation of conflicting type implementations
/// of this trait. It however is not actually needed by any of our code.
#[cfg(feature = "servers")]
pub trait OnResponseStreamBeginHandler<ParamTy, State: Clone + Send + Sync + 'static> {
	type Future: Future<Output = Result<bool, CatBridgeError>> + Send + 'static;

	fn call(self, event: ResponseStreamEvent<State>) -> Self::Future;
}

/// Allow any async function without arguments to be a handler
#[cfg(feature = "servers")]
impl<UnderlyingFnType, FnFutureTy, ResponseTy, State> OnResponseStreamBeginHandler<(), State>
	for UnderlyingFnType
where
	UnderlyingFnType: FnOnce() -> FnFutureTy + Clone + Send + 'static,
	FnFutureTy: Future<Output = ResponseTy> + Send,
	ResponseTy: Into<Result<bool, CatBridgeError>>,
	State: Clone + Send + Sync + 'static,
{
	type Future = Pin<Box<dyn Future<Output = Result<bool, CatBridgeError>> + Send>>;

	fn call(self, _event: ResponseStreamEvent<State>) -> Self::Future {
		Box::pin(async move { self().await.into() })
	}
}

/// Allow any async function with a single consuming argument.
#[cfg(feature = "servers")]
impl<UnderlyingFnType, FnFutureTy, ResponseTy, State, ArgTy>
	OnResponseStreamBeginHandler<ArgTy, State> for UnderlyingFnType
where
	UnderlyingFnType: FnOnce(ArgTy) -> FnFutureTy + Clone + Send + 'static,
	FnFutureTy: Future<Output = ResponseTy> + Send,
	ResponseTy: Into<Result<bool, CatBridgeError>>,
	ArgTy: From<ResponseStreamEvent<State>> + Send,
	State: Clone + Send + Sync + 'static,
{
	type Future = Pin<Box<dyn Future<Output = Result<bool, CatBridgeError>> + Send>>;

	fn call(self, event: ResponseStreamEvent<State>) -> Self::Future {
		Box::pin(async move { self(ArgTy::from(event)).await.into() })
	}
}

macro_rules! fn_to_on_connection_handler {
	(
		[$($ty:ident),*], $last:ident
	) => {
		#[allow(non_snake_case, unused_mut)]
		#[cfg(feature = "clients")]
		impl<UnderlyingFnType, FnFutureTy, OutputTy, State, $($ty,)* $last> OnRequestStreamBeginHandler<($($ty,)* $last,), State> for UnderlyingFnType
		where
			UnderlyingFnType: FnOnce($($ty,)* $last,) -> FnFutureTy + Clone + Send + 'static,
			FnFutureTy: Future<Output = OutputTy> + Send,
			OutputTy: Into<Result<bool, CatBridgeError>>,
			State: Clone + Send + Sync + 'static,
			$( $ty: FromRequestStreamEvent<State> + Send, )*
			$last: FromRequestStreamEvent<State> + Send,
		{
			type Future = Pin<Box<dyn Future<Output = Result<bool, CatBridgeError>> + Send>>;

			fn call(self, mut req: RequestStreamEvent<State>) -> Self::Future {
				Box::pin(async move {
					$(
						let $ty = $ty::from_stream_event(&mut req).await?;
					)*
					let $last = $last::from_stream_event(&mut req).await?;
					let res = self($($ty,)* $last).await;
					res.into()
				})
			}
		}

		#[allow(non_snake_case, unused_mut)]
		#[cfg(feature = "servers")]
		impl<UnderlyingFnType, FnFutureTy, OutputTy, State, $($ty,)* $last> OnResponseStreamBeginHandler<($($ty,)* $last,), State> for UnderlyingFnType
		where
			UnderlyingFnType: FnOnce($($ty,)* $last,) -> FnFutureTy + Clone + Send + 'static,
			FnFutureTy: Future<Output = OutputTy> + Send,
			OutputTy: Into<Result<bool, CatBridgeError>>,
			State: Clone + Send + Sync + 'static,
			$( $ty: FromResponseStreamEvent<State> + Send, )*
			$last: FromResponseStreamEvent<State> + Send,
		{
			type Future = Pin<Box<dyn Future<Output = Result<bool, CatBridgeError>> + Send>>;

			fn call(self, mut req: ResponseStreamEvent<State>) -> Self::Future {
				Box::pin(async move {
					$(
						let $ty = $ty::from_stream_event(&mut req).await?;
					)*
					let $last = $last::from_stream_event(&mut req).await?;
					let res = self($($ty,)* $last).await;
					res.into()
				})
			}
		}
	}
}

fn_to_on_connection_handler!([], T1);
fn_to_on_connection_handler!([T1], T2);
fn_to_on_connection_handler!([T1, T2], T3);
fn_to_on_connection_handler!([T1, T2, T3], T4);
fn_to_on_connection_handler!([T1, T2, T3, T4], T5);
fn_to_on_connection_handler!([T1, T2, T3, T4, T5], T6);
fn_to_on_connection_handler!([T1, T2, T3, T4, T5, T6], T7);
fn_to_on_connection_handler!([T1, T2, T3, T4, T5, T6, T7], T8);
fn_to_on_connection_handler!([T1, T2, T3, T4, T5, T6, T7, T8], T9);
fn_to_on_connection_handler!([T1, T2, T3, T4, T5, T6, T7, T8, T9], T10);
fn_to_on_connection_handler!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10], T11);
fn_to_on_connection_handler!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11], T12);
fn_to_on_connection_handler!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12], T13);
fn_to_on_connection_handler!(
	[T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13],
	T14
);
fn_to_on_connection_handler!(
	[T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14],
	T15
);
fn_to_on_connection_handler!(
	[
		T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15
	],
	T16
);

/// A wrapper around a handler to implement a service.
pub struct OnStreamBeginHandlerAsService<HandlerTy, HandlerParamsTy> {
	handler: HandlerTy,
	marker: PhantomData<HandlerParamsTy>,
}

impl<HandlerTy, HandlerParamsTy> OnStreamBeginHandlerAsService<HandlerTy, HandlerParamsTy> {
	#[must_use]
	pub(crate) fn new(handler: HandlerTy) -> Self {
		Self {
			handler,
			marker: PhantomData,
		}
	}
}

impl<HandlerTy, HandlerParamsTy> Clone for OnStreamBeginHandlerAsService<HandlerTy, HandlerParamsTy>
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

#[cfg(feature = "clients")]
impl<HandlerTy, HandlerParamsTy, State> Service<RequestStreamEvent<State>>
	for OnStreamBeginHandlerAsService<HandlerTy, HandlerParamsTy>
where
	HandlerTy: OnRequestStreamBeginHandler<HandlerParamsTy, State> + Clone + Send + 'static,
	State: Clone + Send + Sync + 'static,
{
	type Response = bool;
	type Error = CatBridgeError;
	type Future = HandlerTy::Future;

	#[inline]
	fn poll_ready(&mut self, _ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		Poll::Ready(Ok(()))
	}

	fn call(&mut self, req: RequestStreamEvent<State>) -> Self::Future {
		let handler = self.handler.clone();
		handler.call(req)
	}
}

#[cfg(feature = "servers")]
impl<HandlerTy, HandlerParamsTy, State> Service<ResponseStreamEvent<State>>
	for OnStreamBeginHandlerAsService<HandlerTy, HandlerParamsTy>
where
	HandlerTy: OnResponseStreamBeginHandler<HandlerParamsTy, State> + Clone + Send + 'static,
	State: Clone + Send + Sync + 'static,
{
	type Response = bool;
	type Error = CatBridgeError;
	type Future = HandlerTy::Future;

	#[inline]
	fn poll_ready(&mut self, _ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		Poll::Ready(Ok(()))
	}

	fn call(&mut self, req: ResponseStreamEvent<State>) -> Self::Future {
		let handler = self.handler.clone();
		handler.call(req)
	}
}
