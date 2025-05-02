//! Models specifically for the TCP Client, as opposed to the TCP Server.

use crate::{
	errors::CatBridgeError,
	net::{Extensions, models::Request},
};
use fnv::FnvHasher;
use std::{
	fmt::{Debug, Formatter, Result as FmtResult},
	hash::{Hash, Hasher},
	net::SocketAddr,
};
use tokio::{sync::mpsc::Sender, task::Builder as TaskBuilder};
use tower::{Service, util::BoxCloneService};
use tracing::warn;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// A channel to send messages to a connection from an "out of bound"
/// location.
///
/// This way you could do things like health check, or PUB/SUB style
/// architectures.
#[derive(Clone, Debug, Valuable)]
pub enum RequestStreamMessage<State: Clone + Send + Sync + 'static = ()> {
	/// Request a disconnection without any response.
	Disconnect,
	/// Send an actual response back out.
	Request(Request<State>),
}

/// An event that is sent when a new connection is created, or destroyed.
pub struct RequestStreamEvent<State: Clone + Send + Sync + 'static = ()> {
	/// A channel to send responses out-of-band responses on.
	///
	/// Allows for things like health-checking, and broadcasts of friend messages
	/// out-of-band.
	connection_channel: Option<Sender<RequestStreamMessage>>,
	/// The list of extensions to attach to this event.
	ext: Extensions,
	/// The source address of the initiator
	source_address: SocketAddr,
	/// A unique stream id.
	///
	/// On UDP where we don't have streams this is based off of the source-address.
	stream_id: Option<u64>,
	/// The app state on the current connection.
	state: State,
}

impl RequestStreamEvent<()> {
	#[must_use]
	pub const fn new(
		connection_channel: Sender<RequestStreamMessage>,
		source_address: SocketAddr,
		stream_id: Option<u64>,
	) -> Self {
		Self::new_with_state(connection_channel, source_address, stream_id, ())
	}

	#[must_use]
	pub const fn new_disconnected(source_address: SocketAddr, stream_id: Option<u64>) -> Self {
		Self::new_disconnected_with_state(source_address, stream_id, ())
	}
}

impl<State: Clone + Send + Sync + 'static> RequestStreamEvent<State> {
	#[must_use]
	pub const fn new_with_state(
		connection_channel: Sender<RequestStreamMessage>,
		source_address: SocketAddr,
		stream_id: Option<u64>,
		state: State,
	) -> Self {
		Self {
			connection_channel: Some(connection_channel),
			ext: Extensions::new(),
			source_address,
			stream_id,
			state,
		}
	}

	#[must_use]
	pub const fn new_disconnected_with_state(
		source_address: SocketAddr,
		stream_id: Option<u64>,
		state: State,
	) -> Self {
		Self {
			connection_channel: None,
			ext: Extensions::new(),
			source_address,
			stream_id,
			state,
		}
	}

	/// A unique identifier for the "stream" or connection of a packet.
	///
	/// In UDP which doesn't have stream this uses the source address as
	/// the core identifier.
	#[must_use]
	pub fn stream_id(&self) -> u64 {
		if let Some(id) = self.stream_id {
			id
		} else {
			let mut hasher = FnvHasher::default();
			self.source_address.hash(&mut hasher);
			hasher.finish()
		}
	}

	/// Allows retrieving a channel to send out-of-band packets to a connection.
	///
	/// This type is clone-able, and can be used without needing to hold onto this event.
	#[must_use]
	pub const fn out_of_band_channel(&self) -> Option<&Sender<RequestStreamMessage>> {
		self.connection_channel.as_ref()
	}

	#[must_use]
	pub const fn state(&self) -> &State {
		&self.state
	}
	#[must_use]
	pub fn state_mut(&mut self) -> &mut State {
		&mut self.state
	}

	#[must_use]
	pub const fn extensions(&self) -> &Extensions {
		&self.ext
	}
	#[must_use]
	pub fn extensions_mut(&mut self) -> &mut Extensions {
		&mut self.ext
	}

	#[must_use]
	pub const fn source(&self) -> &SocketAddr {
		&self.source_address
	}
	#[must_use]
	pub fn is_ipv4(&self) -> bool {
		self.source_address.ip().is_ipv4()
	}
	#[must_use]
	pub fn is_ipv6(&self) -> bool {
		self.source_address.ip().is_ipv6()
	}
}

impl<State: Clone + Send + Sync + 'static> Debug for RequestStreamEvent<State> {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		fmt.debug_struct("RequestStreamEvent")
			// Extensions can't be printed in debug by hyper, and in order to keep
			// compatability ours don't.
			.field("source_address", &self.source_address)
			.field("stream_id", &self.stream_id)
			.finish_non_exhaustive()
	}
}

const REQUEST_STREAM_EVENT_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("source_address"),
	NamedField::new("stream_id"),
];

impl<State: Clone + Send + Sync + 'static> Structable for RequestStreamEvent<State> {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"RequestStreamEvent",
			Fields::Named(REQUEST_STREAM_EVENT_FIELDS),
		)
	}
}

impl<State: Clone + Send + Sync + 'static> Valuable for RequestStreamEvent<State> {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			REQUEST_STREAM_EVENT_FIELDS,
			&[
				Valuable::as_value(&format!("{}", self.source_address)),
				Valuable::as_value(&self.stream_id),
			],
		));
	}
}

/// The underlying type we use for storing your on connection handler.
pub type UnderlyingOnStreamBeginService<State> =
	BoxCloneService<RequestStreamEvent<State>, bool, CatBridgeError>;
/// The underlying type we use for storing your on disconnect handler.
pub type UnderlyingOnStreamEndService<State> =
	BoxCloneService<RequestStreamEvent<State>, (), CatBridgeError>;

/// Extract any value from a Connection event.
///
/// Mirrors [`crate::net::models::FromRequest`].
pub trait FromRequestStreamEvent<State: Clone + Send + Sync + 'static>: Sized {
	fn from_stream_event(
		event: &mut RequestStreamEvent<State>,
	) -> impl Future<Output = Result<Self, CatBridgeError>> + Send;
}

/// A type that holds onto a [`Service`], and can call it on drop.
///
/// This spawns a temporary task to run the async processing, and moves the
/// event data within.
pub(crate) struct DisconnectAsyncDropClient<
	ServiceTy: Clone
		+ Service<
			RequestStreamEvent<State>,
			Future = ServiceFutureTy,
			Response = (),
			Error = CatBridgeError,
		> + Send
		+ 'static,
	ServiceFutureTy: Future<Output = Result<(), CatBridgeError>> + Send,
	State: Clone + Send + Sync + 'static,
> {
	service: ServiceTy,
	state: State,
	source_address: SocketAddr,
	stream_id: u64,
}

impl<
	ServiceTy: Clone
		+ Service<
			RequestStreamEvent<State>,
			Future = ServiceFutureTy,
			Response = (),
			Error = CatBridgeError,
		> + Send
		+ 'static,
	ServiceFutureTy: Future<Output = Result<(), CatBridgeError>> + Send,
	State: Clone + Send + Sync + 'static,
> DisconnectAsyncDropClient<ServiceTy, ServiceFutureTy, State>
{
	#[must_use]
	pub const fn new(
		service: ServiceTy,
		state: State,
		source_address: SocketAddr,
		stream_id: u64,
	) -> Self {
		Self {
			service,
			state,
			source_address,
			stream_id,
		}
	}
}

impl<
	ServiceTy: Clone
		+ Service<
			RequestStreamEvent<State>,
			Future = ServiceFutureTy,
			Response = (),
			Error = CatBridgeError,
		> + Send
		+ 'static,
	ServiceFutureTy: Future<Output = Result<(), CatBridgeError>> + Send,
	State: Clone + Send + Sync + 'static,
> Drop for DisconnectAsyncDropClient<ServiceTy, ServiceFutureTy, State>
{
	fn drop(&mut self) {
		let addr = self.source_address;
		let mut svc = self.service.clone();
		let state = self.state.clone();
		let stream_id = self.stream_id;

		if let Err(cause) = TaskBuilder::new().name("cat_dev::net::client::models::DisconnectAsyncDrop").spawn(async move {
			if let Err(cause) = svc.call(
				RequestStreamEvent::new_disconnected_with_state(addr, Some(stream_id), state),
			).await {
				warn!(
					?cause,
					client.address = %addr,
					server.stream_id = stream_id,
					"On stream end task has failed during it's processing, and may need to be cleaned up manually.",
				);
			}
		}) {
			warn!(
				?cause,
				client.address = %addr,
				server.stream_id = stream_id,
				"On Stream end task has failed to be spawned, and will not be completed!",
			);
		}
	}
}
