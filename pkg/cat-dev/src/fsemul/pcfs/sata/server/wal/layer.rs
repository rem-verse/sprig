//! Allow attaching the Write-Ahead log as an arbitrary layer to any server.

use crate::{
	errors::CatBridgeError,
	fsemul::pcfs::sata::server::wal::WriteAheadLog,
	net::{
		models::{Request, Response},
		server::models::ResponseStreamEvent,
	},
};
use std::{
	convert::Infallible,
	pin::Pin,
	task::{Context, Poll},
};
use tower::{Layer, Service};

/// A layer that will automatically record begin streams from a
/// TCP Server.
#[derive(Clone, Debug)]
pub struct WALBeginStreamLayer(pub WriteAheadLog);

impl<Layered> Layer<Layered> for WALBeginStreamLayer
where
	Layered: Clone,
{
	type Service = LayeredBeginWALStream<Layered>;

	fn layer(&self, inner: Layered) -> Self::Service {
		LayeredBeginWALStream {
			inner,
			log: self.0.clone(),
		}
	}
}

#[derive(Clone)]
pub struct LayeredBeginWALStream<Layered> {
	inner: Layered,
	log: WriteAheadLog,
}

impl<Layered, State: Clone + Send + Sync + 'static> Service<ResponseStreamEvent<State>>
	for LayeredBeginWALStream<Layered>
where
	Layered: Service<ResponseStreamEvent<State>, Response = bool, Error = CatBridgeError>
		+ Clone
		+ Send
		+ 'static,
	Layered::Future: Send + 'static,
{
	type Response = Layered::Response;
	type Error = Layered::Error;
	type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

	#[inline]
	fn poll_ready(&mut self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.inner.poll_ready(ctx)
	}

	fn call(&mut self, evt: ResponseStreamEvent<State>) -> Self::Future {
		let log = self.log.clone();
		let mut inner = self.inner.clone();

		Box::pin(async move {
			log.record_open_stream(evt.stream_id()).await;
			inner.call(evt).await
		})
	}
}

/// A layer that will automatically record end streams from a
/// TCP Server.
#[derive(Clone, Debug)]
pub struct WALEndStreamLayer(pub WriteAheadLog);

impl<Layered> Layer<Layered> for WALEndStreamLayer
where
	Layered: Clone,
{
	type Service = LayeredEndWALStream<Layered>;

	fn layer(&self, inner: Layered) -> Self::Service {
		LayeredEndWALStream {
			inner,
			log: self.0.clone(),
		}
	}
}

#[derive(Clone)]
pub struct LayeredEndWALStream<Layered> {
	inner: Layered,
	log: WriteAheadLog,
}

impl<Layered, State: Clone + Send + Sync + 'static> Service<ResponseStreamEvent<State>>
	for LayeredEndWALStream<Layered>
where
	Layered: Service<ResponseStreamEvent<State>, Response = (), Error = CatBridgeError>
		+ Clone
		+ Send
		+ 'static,
	Layered::Future: Send + 'static,
{
	type Response = Layered::Response;
	type Error = Layered::Error;
	type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

	#[inline]
	fn poll_ready(&mut self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.inner.poll_ready(ctx)
	}

	fn call(&mut self, evt: ResponseStreamEvent<State>) -> Self::Future {
		let log = self.log.clone();
		let mut inner = self.inner.clone();

		Box::pin(async move {
			log.record_close_stream(evt.stream_id()).await;
			inner.call(evt).await
		})
	}
}

/// A layer that will automatically record requests/responses, and attach a WAL
/// as an extension.
#[derive(Clone, Debug)]
pub struct WALMessageLayer(pub WriteAheadLog);

impl<Layered> Layer<Layered> for WALMessageLayer
where
	Layered: Clone,
{
	type Service = LayeredWALMessage<Layered>;

	fn layer(&self, inner: Layered) -> Self::Service {
		LayeredWALMessage {
			inner,
			log: self.0.clone(),
		}
	}
}

#[derive(Clone)]
pub struct LayeredWALMessage<Layered> {
	inner: Layered,
	log: WriteAheadLog,
}

impl<Layered, State: Clone + Send + Sync + 'static> Service<Request<State>>
	for LayeredWALMessage<Layered>
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
		let log = self.log.clone();
		let mut inner = self.inner.clone();

		Box::pin(async move {
			let sid = req.stream_id();
			log.record_request(sid, req.body().clone()).await;
			req.extensions_mut().insert(log.clone());
			match inner.call(req).await {
				Ok(resp) => {
					if let Some(bod) = resp.body() {
						log.record_response(sid, bod.clone()).await;
					}
					Ok::<Layered::Response, Layered::Error>(resp)
				}
				Err(cause) => Err::<Layered::Response, Layered::Error>(cause),
			}
		})
	}
}
