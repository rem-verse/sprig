//! A Router capable of routing on arbitrary byte sequences, aka TCP/UDP
//! packets.
//!
//! ## Route Matching ##
//!
//! Route matching in the TCP world has some differences compared to what most
//! folks are used to writing routers for, HTTP. More specifically we need to
//! parse the ENTIRE PACKET, in order to know where to route too.
//!
//! The key difference here is you can't for example, match a route, and then
//! parse the body once you know where to route too. As such parsing is fully
//! seperate from routing.
//!
//! ### Routing ###
//!
//! Routing is based off a concept of "narrowing". Narrowing is a way of
//! identifying packets based off of ID/Data at a particular position in a
//! packet. Our particular "narrowing" approach supports matching up to 16
//! bytes at once (ssupports every int size in rust even `u128`!) while also
//! being widely available for SIMD.
//!
//! Narrow'ing itself for us is actually just two steps:
//!
//! 1. Identify the first byte we want to use to determine routability.
//!
//!   - Some services only use a single byte as a packet identifier, so we
//!     get lucky, and can just route directly to that.
//!   - 16 bytes were chosen as a happy middle ground to be large enough to
//!     be useful (supports up to the highest size rust type u128 as an
//!     identifier), while also still being widely implemented in terms of SIMD,
//!     or NEON to the point of being practically faster.
//!
//! ***Assumptions that may break for you, causing this to not work well/at
//! all:***
//!
//! 1. You have data that can't be identified by just 16 raw bytes. This makes
//!    this router a non-starter for you. We only support matching up to 16 bytes.
//!    The main reason for this is to benefit from SIMD where we can see
//!    improvements of up to 300x in speed.
//! 2. You have to run on a machine where SIMD is not readily available, many
//!    features may be slower if they aren't using parallelized SIMD variants.
//!
//! ## Usage ##
//!
//! The usage of this router is almost the exact same as an axum handler for
//! example it might look like this:
//!
//! ```rust
//! use bytes::Bytes;
//! use cat_dev::net::server::{requestable::*, Router};
//!
//! async fn echo_extension(
//!   Extension(data): Extension<String>,
//!   Body(as_bytes): Body<Bytes>,
//! ) -> String {
//!   "a static result".to_owned()
//! }
//!
//! let mut router = Router::<()>::new();
//! // Capture any packets that start with `0x1`.
//! router.add_route(&[0x1], echo_extension).expect("Failed to add route!");
//! router.layer(Extension::<String>("An extension to share with requests".to_owned()));
//! // The router is now ready to use!
//! ```
//!
//! ## Backpressure Notes ##
//!
//! Generally routing to one of multiple services and backpressure doesn't mix well. Ideally you
//! would want to ensure a service is ready to receive a request before calling it. However, in
//! order to know which service to call, you need the request...
//!
//! One approach is to not consider the router service itself ready until all destination services
//! are ready. This is the approach used by [`tower::steer::Steer`].
//!
//! Another approach is to always consider all services read (always `return Poll::Ready(Ok(()))`)
//! from `Service::poll_ready` and then and then actually drive readiness inside the response
//! future returned by `Service::call`. This works well when your services don't care about
//! backpressure and are always ready anyway.
//!
//! This utility expects that all services used in your app wont care about backpressure and so
//! it usess the latter strategy. However, that means you should avoid routing to a service (or
//! using a middleware) that does care about backpressure. At the very least you should load shed
//! so requests are dropped quickly and don't keep piling on.
//!
//! It also means that if `poll_ready` returns an error then that error will be returned in
//! the response future from call and not from `poll_ready`. In that case, the underlying
//! service will not be discarded and will continue to be used for future requests.
//!
//! Services that expect to be discarded if `poll_ready` fails should not be used with this router.

use crate::{
	errors::CatBridgeError,
	net::{
		errors::CommonNetAPIError,
		models::{Request, Response},
		server::request_handlers::{Handler, HandlerAsService},
	},
};
use fnv::FnvHashMap;
use std::{
	convert::Infallible,
	hash::BuildHasherDefault,
	pin::Pin,
	task::{Context, Poll},
};
use tower::{Layer, Service, util::BoxCloneService};
use tracing::{debug, warn};
use wide::u8x16;

type RoutableService<State> = BoxCloneService<Request<State>, Response, CatBridgeError>;
type RoutingInfo = Vec<(u8x16, Vec<u8>)>;

/// A router that is capable of routing based on the prefixes of Layer 4
/// packets.
///
/// See the module documentation for more information about how exactly the
/// routing happens, how backpressure is handled, and how narrowing down
/// packets.
///
/// Routes themselves are arbitrary [`tower::Service`]'s, where the crate
/// provides automatic conversions from `async fn` into services.
#[derive(Clone, Debug)]
pub struct Router<State: Clone + Send + Sync + 'static = ()> {
	/// The service to call if an unknown route is being handled.
	fallback: Option<RoutableService<State>>,
	/// The offset the "packet id" is at, if it is not the beginning.
	offset_at: Option<usize>,
	/// The underlying route-table for this particular router.
	///
	/// The map is:
	///
	/// `(first_byte, (list_of_matchers, list_of_service_to_route_too))`
	///
	/// We keep these as two separate lists so the contigous nature of matchers
	/// is the best for cpu cache hits, even if it requires more space over head.
	route_table: FnvHashMap<u8, (RoutingInfo, Vec<RoutableService<State>>)>,
}

impl<State: Clone + Send + Sync + 'static> Router<State> {
	/// Create a new prefix routable router.
	#[must_use]
	pub fn new() -> Self {
		Self {
			fallback: None,
			offset_at: None,
			route_table: FnvHashMap::with_capacity_and_hasher(0, BuildHasherDefault::default()),
		}
	}

	/// Create a new prefix routable router who treats a packet id at a particular offset.
	#[must_use]
	pub fn new_with_offset(offset_at: usize) -> Self {
		Self {
			fallback: None,
			offset_at: Some(offset_at),
			route_table: FnvHashMap::with_capacity_and_hasher(0, BuildHasherDefault::default()),
		}
	}

	/// Add a route to be handled by the router, this accepts a [`Handler`] which
	/// most of the time is an async function.
	///
	/// This takes a `packet_start` which is a set of at most 16 bytes that can
	/// be matched against the configured byte offset of a packet to determine
	/// matching.
	///
	/// ## Errors
	///
	/// If the `packet_start` is already greater than 16 bytes, empty, or the route
	/// has already been registered.
	pub fn add_route<HandlerTy, HandlerParamsTy>(
		&mut self,
		packet_start: &[u8],
		handle: HandlerTy,
	) -> Result<(), CommonNetAPIError>
	where
		HandlerTy: Handler<HandlerParamsTy, State> + Clone + Send + 'static,
		HandlerParamsTy: Send + 'static,
	{
		let boxed = BoxCloneService::new(HandlerAsService::new(handle));
		let widened = Self::to_wide_simd(packet_start)?;

		if let Some(routes) = self.route_table.get_mut(&packet_start[0]) {
			for (wide, _) in &routes.0 {
				if *wide == widened {
					return Err(CommonNetAPIError::DuplicateRoute(Vec::from(packet_start)));
				}
			}
			routes.0.push((widened, Vec::from(packet_start)));
			routes.1.push(boxed);
		} else {
			self.route_table.insert(
				packet_start[0],
				(vec![(widened, Vec::from(packet_start))], vec![boxed]),
			);
		}

		Ok(())
	}

	/// Add a route to be processed for anything that implements [`Service`].
	///
	/// This allows applying more generic tower services to the router directly.
	/// We expect the general case to still pass in functions which end up going
	/// to the [`add_route`]. AS the function gets cast to a handler.
	///
	/// ## Errors
	///
	/// If the `packet_start` is longer than 16 bytes long, or this route has
	/// already been registered.
	pub fn add_route_service<ServiceTy>(
		&mut self,
		packet_start: &[u8],
		service: ServiceTy,
	) -> Result<(), CommonNetAPIError>
	where
		ServiceTy: Service<Request<State>, Response = Response, Error = CatBridgeError>
			+ Clone
			+ Send
			+ 'static,
		ServiceTy::Future: Send,
	{
		let boxed = BoxCloneService::new(service);
		let widened = Self::to_wide_simd(packet_start)?;

		if let Some(routes) = self.route_table.get_mut(&packet_start[0]) {
			for (wide, _) in &routes.0 {
				if *wide == widened {
					return Err(CommonNetAPIError::DuplicateRoute(Vec::from(packet_start)));
				}
			}
			routes.0.push((widened, Vec::from(packet_start)));
			routes.1.push(boxed);
		} else {
			self.route_table.insert(
				packet_start[0],
				(vec![(widened, Vec::from(packet_start))], vec![boxed]),
			);
		}

		Ok(())
	}

	/// Apply a layer to all _existing_ routes that currently exist in the router.
	///
	/// This as a result will not apply to future routes added to this router,
	/// and the last applied layers will run first. This may seem confusing at
	/// first but basically works the same way as in axum.
	pub fn layer<LayerTy, ServiceTy>(&mut self, layer: LayerTy)
	where
		LayerTy:
			Layer<BoxCloneService<Request<State>, Response, CatBridgeError>, Service = ServiceTy>,
		ServiceTy: Service<Request<State>, Response = Response, Error = CatBridgeError>
			+ Clone
			+ Send
			+ 'static,
		<LayerTy::Service as Service<Request<State>>>::Future: Send + 'static,
	{
		for route_table in self.route_table.values_mut() {
			for route in &mut route_table.1 {
				*route = BoxCloneService::new(layer.layer(route.clone()));
			}
		}
	}

	/// Set the handler to call when a route cannot be found.
	///
	/// ## Errors
	///
	/// If there is already a fallback handler registered.
	pub fn fallback_handler<HandlerTy, HandlerParamsTy>(
		&mut self,
		handle: HandlerTy,
	) -> Result<(), CommonNetAPIError>
	where
		HandlerTy: Handler<HandlerParamsTy, State> + Clone + Send + 'static,
		HandlerParamsTy: Send + 'static,
	{
		if self.fallback.is_some() {
			return Err(CommonNetAPIError::DuplicateFallbackHandler);
		}
		self.fallback = Some(BoxCloneService::new(HandlerAsService::new(handle)));
		Ok(())
	}

	/// Set the handler to call when a route cannot be found.
	///
	/// ## Errors
	///
	/// If there is already a fallback handler registered.
	pub fn fallback_handler_service<ServiceTy>(
		&mut self,
		service: ServiceTy,
	) -> Result<(), CommonNetAPIError>
	where
		ServiceTy: Service<Request<State>, Response = Response, Error = CatBridgeError>
			+ Clone
			+ Send
			+ 'static,
		ServiceTy::Future: Send,
	{
		if self.fallback.is_some() {
			return Err(CommonNetAPIError::DuplicateFallbackHandler);
		}
		self.fallback = Some(BoxCloneService::new(service));
		Ok(())
	}

	fn to_wide_simd(pre: &[u8]) -> Result<u8x16, CommonNetAPIError> {
		if pre.is_empty() {
			return Err(CommonNetAPIError::RouterNeedsSomeBytesToMatchOn);
		}
		if pre.len() > 16 {
			return Err(CommonNetAPIError::RouteTooLongToMatchOn(Vec::from(pre)));
		}

		let mut data = [0_u8; 16];
		for (idx, byte) in pre.iter().enumerate() {
			data[idx] = *byte;
		}
		Ok(u8x16::new(data))
	}

	fn to_wide_simd_offset(packet: &[u8], offset: usize) -> u8x16 {
		let mut data = [0_u8; 16];
		data[..std::cmp::min(packet.len() - offset, 16)]
			.copy_from_slice(&packet[offset..offset + std::cmp::min(packet.len() - offset, 16)]);
		u8x16::new(data)
	}
}

impl<State: Clone + Send + Sync + 'static> Default for Router<State> {
	fn default() -> Self {
		Self::new()
	}
}

impl<State: Clone + Send + Sync + 'static> Service<Request<State>> for Router<State> {
	type Response = Response;
	type Error = Infallible;
	type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

	fn poll_ready(&mut self, _ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		Poll::Ready(Ok(()))
	}

	fn call(&mut self, req: Request<State>) -> Self::Future {
		let mut handler = None;

		// If body is empty, or not long enough..., call fallback immediately router needs bytes.
		let body = req.body();
		let offset = self.offset_at.unwrap_or_default();
		if offset >= body.len() {
			handler.clone_from(&self.fallback);
		} else if let Some(possible_routes) = self.route_table.get(&body[offset]) {
			let padded = Self::to_wide_simd_offset(body, offset);

			for (idx, (possible_route, array)) in possible_routes.0.iter().enumerate() {
				let mut result = u8x16::new([0_u8; 16]);
				result |= possible_route;
				result |= padded;
				if result == padded && body[offset..].starts_with(array) {
					handler = Some(possible_routes.1[idx].clone());
					break;
				}
			}

			if handler.is_none() {
				handler.clone_from(&self.fallback);
			}
		} else {
			handler.clone_from(&self.fallback);
		}

		Box::pin(async move {
			if handler.is_none() {
				debug!(
					request.body = format!("{:02x?}", req.body()),
					"unknown handler called for router!",
				);
				return Ok(Response::new_empty());
			}
			let mut hndl = handler.unwrap();

			match hndl.call(req).await {
				Ok(resp) => Ok(resp),
				Err(cause) => {
					warn!(?cause, "handler for l4 router failed");
					Ok(Response::empty_close())
				}
			}
		})
	}
}

#[cfg(test)]
pub mod test_helpers {
	#![allow(unused)]

	use super::*;
	use bytes::Bytes;
	use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

	/// Assert that a router being called with a packet returns no value
	/// successfully (e.g. without a close).
	pub async fn router_empty_no_close(router: &mut Router, packet: &'static [u8]) {
		router_empty_no_close_with_state(router, packet, ()).await
	}

	/// Assert that a router being called with a packet returns no value
	/// successfully (e.g. without a close).
	pub async fn router_empty_no_close_with_state<State: Clone + Send + Sync + 'static>(
		router: &mut Router<State>,
		packet: &'static [u8],
		state: State,
	) {
		let resp = router
			.call(Request::new_with_state(
				Bytes::from_static(packet),
				SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
				state,
				None,
			))
			.await
			.expect("Failed to call router!");

		assert!(
			!resp.request_connection_close(),
			"Response indiciated that it should be closed.",
		);

		if let Some(body) = resp.body() {
			assert!(
				body.is_empty(),
				"Response body was not empty! Was:\n  hex: {:02x?}\n  str: {}",
				body,
				String::from_utf8_lossy(&body),
			);
		}
	}

	/// Call the router, and return it's response after asserting that it did
	/// not send us a connection close.
	pub async fn router_body_no_close(router: &mut Router, packet: &'static [u8]) -> Vec<u8> {
		router_body_no_close_with_state(router, packet, ()).await
	}

	/// Call the router, and return it's response after asserting that it did
	/// not send us a connection close.
	pub async fn router_body_no_close_with_state<State: Clone + Send + Sync + 'static>(
		router: &mut Router<State>,
		packet: &'static [u8],
		state: State,
	) -> Vec<u8> {
		let resp = router
			.call(Request::new_with_state(
				Bytes::from_static(packet),
				SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
				state,
				None,
			))
			.await
			.expect("Failed to call router!");

		assert!(
			!resp.request_connection_close(),
			"Response indiciated that it should be closed.",
		);

		resp.body()
			.expect("Failed to find response body from called route!")
			.iter()
			.map(|i| *i)
			.collect::<Vec<_>>()
	}

	/// Assert that a router "error'd", e.g. closes without a body.
	pub async fn router_empty_close(router: &mut Router, packet: &'static [u8]) {
		router_empty_close_with_state(router, packet, ()).await
	}

	pub async fn router_empty_close_with_state<State: Clone + Send + Sync + 'static>(
		router: &mut Router<State>,
		packet: &'static [u8],
		state: State,
	) {
		let resp = router
			.call(Request::new_with_state(
				Bytes::from_static(packet),
				SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
				state,
				None,
			))
			.await
			.expect("Failed to call router!");

		assert!(resp.request_connection_close());
		assert!(resp.body().is_none());
	}
}

#[cfg(test)]
mod unit_tests {
	use super::{test_helpers::*, *};
	use crate::net::server::requestable::State;
	use bytes::{Bytes, BytesMut};
	use std::sync::{
		Arc,
		atomic::{AtomicU8, Ordering},
	};

	#[tokio::test]
	pub async fn route_to_handler_full() {
		let mut router = Router::new();
		router
			.add_route(&[0x3, 0x4, 0x5, 0x6, 0x7], || async {
				Ok::<Response, CatBridgeError>(Response::from(Bytes::from_static(b"hello world")))
			})
			.expect("Failed to add route!");

		assert_eq!(
			router_body_no_close(
				&mut router,
				&[0x3, 0x4, 0x5, 0x6, 0x7, 0x8, 0x9, 0x10, 0x11, 0x12]
			)
			.await,
			b"hello world"
		);
	}

	#[tokio::test]
	pub async fn route_to_handler_into() {
		let mut router = Router::new();

		async fn static_byte() -> &'static [u8] {
			b"hello world static"
		}

		router
			.add_route(&[0x1], || async { Bytes::from_static(b"hello world") })
			.expect("Failed to add route!");
		router
			.add_route(&[0x2], || async {
				let mut bytes = BytesMut::with_capacity(0);
				bytes.extend_from_slice(b"hello world bytes_mut");
				bytes
			})
			.expect("Failed to add route!");
		router
			.add_route(&[0x3], || async { "hello world string".to_owned() })
			.expect("Failed to add route!");
		router
			.add_route(&[0x4], || async {
				b"hello world vec".iter().map(|i| *i).collect::<Vec<u8>>()
			})
			.expect("Failed to add route!");
		router
			.add_route(&[0x5], static_byte)
			.expect("Failed to add route!");
		router
			.add_route(&[0x6], || async { "hello world static str" })
			.expect("Failed to add route!");
		router
			.add_route(&[0x7], || async {
				Err::<String, CatBridgeError>(CatBridgeError::UnsupportedBitsPerCore)
			})
			.expect("Failed to add route!");

		assert_eq!(
			router_body_no_close(&mut router, &[0x1, 0x2, 0x3]).await,
			b"hello world",
		);
		assert_eq!(
			router_body_no_close(&mut router, &[0x2, 0x2, 0x3]).await,
			b"hello world bytes_mut",
		);
		assert_eq!(
			router_body_no_close(&mut router, &[0x3, 0x2, 0x3]).await,
			b"hello world string",
		);
		assert_eq!(
			router_body_no_close(&mut router, &[0x4, 0x2, 0x3]).await,
			b"hello world vec",
		);
		assert_eq!(
			router_body_no_close(&mut router, &[0x5, 0x2, 0x3]).await,
			b"hello world static",
		);
		assert_eq!(
			router_body_no_close(&mut router, &[0x6, 0x2, 0x3]).await,
			b"hello world static str",
		);

		router_empty_close(&mut router, &[0x7, 0x2, 0x3]).await;
	}

	#[tokio::test]
	pub async fn route_to_fallback() {
		async fn base_handler() -> Result<Bytes, CatBridgeError> {
			Ok(Bytes::from_static(b"base"))
		}

		let mut router = Router::new();
		router
			.add_route(&[0x1, 0x2, 0x3], base_handler)
			.expect("Failed to add!");
		router
			.add_route(&[0x2, 0x2, 0x3], base_handler)
			.expect("Failed to add!");
		router
			.add_route(&[0x3, 0x2, 0x3], base_handler)
			.expect("Failed to add!");
		router
			.add_route(&[0x4, 0x2, 0x3], base_handler)
			.expect("Failed to add!");
		router
			.add_route(&[0x6, 0x2, 0x3], base_handler)
			.expect("Failed to add!");
		router
			.add_route(&[0x7, 0x2, 0x3], base_handler)
			.expect("Failed to add!");
		router
			.add_route(&[0x8, 0x2, 0x3], base_handler)
			.expect("Failed to add!");
		router
			.add_route(&[0x9, 0x2, 0x3], base_handler)
			.expect("Failed to add!");
		router
			.fallback_handler(|| async { Ok(Bytes::from_static(b"fallback")) })
			.expect("Failed to register fallback_handler!");

		assert_eq!(
			router_body_no_close(&mut router, &[0x5, 0x6, 0x7]).await,
			b"fallback"
		);
		assert_eq!(
			router_body_no_close(&mut router, &[0x1, 0x6, 0x7]).await,
			b"fallback"
		);
		assert_eq!(
			router_body_no_close(&mut router, &[0x1, 0x2, 0x3]).await,
			b"base"
		);
	}

	#[tokio::test]
	pub async fn test_route_at_offset() {
		#[derive(Clone)]
		struct TestState {
			fallbacks: Arc<AtomicU8>,
			requests: Arc<AtomicU8>,
		}

		let my_state = TestState {
			fallbacks: Arc::new(AtomicU8::new(0)),
			requests: Arc::new(AtomicU8::new(0)),
		};

		async fn hit(State(test_state): State<TestState>) -> Bytes {
			test_state.requests.fetch_add(1, Ordering::SeqCst);
			Bytes::from_static(b"hewwo mw pwizzwa mwan")
		}
		async fn fallback(State(test_state): State<TestState>) -> Bytes {
			test_state.fallbacks.fetch_add(1, Ordering::SeqCst);
			Bytes::from_static(b"bye pwizza man")
		}

		let mut router = Router::<TestState>::new_with_offset(4);
		router
			.add_route(&[0x1], hit)
			.expect("Failed to add route to router!");
		router
			.add_route(&[0x2, 0x3, 0x4], hit)
			.expect("Failed to add route to router!");
		router
			.add_route(&[0x5, 0x6, 0x7], hit)
			.expect("Failed to add route to router!");
		router
			.fallback_handler(fallback)
			.expect("Failed to set fallback handler!");

		// Not enough bytes, means we call fallback.
		_ = router_body_no_close_with_state(&mut router, &[], my_state.clone()).await;
		_ = router_body_no_close_with_state(&mut router, &[0x1; 4], my_state.clone()).await;
		// These should hit the handler.
		_ = router_body_no_close_with_state(&mut router, &[0x1; 5], my_state.clone()).await;
		_ = router_body_no_close_with_state(
			&mut router,
			&[0x1, 0x1, 0x1, 0x1, 0x2, 0x3, 0x4],
			my_state.clone(),
		)
		.await;
		_ = router_body_no_close_with_state(
			&mut router,
			&[0x1, 0x1, 0x1, 0x1, 0x5, 0x6, 0x7],
			my_state.clone(),
		)
		.await;
		// These should hit 'fallback', cause the routes don't exist.
		_ = router_body_no_close_with_state(
			&mut router,
			&[0x1, 0x1, 0x1, 0x1, 0x9],
			my_state.clone(),
		)
		.await;

		assert_eq!(
			my_state.fallbacks.load(Ordering::SeqCst),
			3,
			"Fallback did not get hit required amount of times!",
		);
		assert_eq!(
			my_state.requests.load(Ordering::SeqCst),
			3,
			"Request Handler did not get hit required amount of times!",
		);
	}
}
