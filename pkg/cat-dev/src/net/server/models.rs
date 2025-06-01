//! Models specifically for the TCP Server, as opposed to the TCP Client.

use crate::{
	errors::CatBridgeError,
	net::{Extensions, models::Response},
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
pub enum ResponseStreamMessage {
	/// Request a disconnection without any response.
	Disconnect,
	/// Send an actual response back out.
	Response(Response),
}

/// An event that is sent when a new connection is created, or destroyed.
pub struct ResponseStreamEvent<State: Clone + Send + Sync + 'static = ()> {
	/// A channel to send responses out-of-band responses on.
	///
	/// Allows for things like health-checking, and broadcasts of friend messages
	/// out-of-band.
	connection_channel: Option<Sender<ResponseStreamMessage>>,
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

impl ResponseStreamEvent<()> {
	#[must_use]
	pub const fn new(
		connection_channel: Sender<ResponseStreamMessage>,
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

impl<State: Clone + Send + Sync + 'static> ResponseStreamEvent<State> {
	#[must_use]
	pub const fn new_with_state(
		connection_channel: Sender<ResponseStreamMessage>,
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
	pub const fn out_of_band_channel(&self) -> Option<&Sender<ResponseStreamMessage>> {
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

impl<State: Clone + Send + Sync + 'static> Debug for ResponseStreamEvent<State> {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		fmt.debug_struct("ResponseStreamEvent")
			// Extensions can't be printed in debug by hyper, and in order to keep
			// compatability ours don't.
			.field("source_address", &self.source_address)
			.field("stream_id", &self.stream_id)
			.finish_non_exhaustive()
	}
}

const CONNECTION_EVENT_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("source_address"),
	NamedField::new("stream_id"),
];

impl<State: Clone + Send + Sync + 'static> Structable for ResponseStreamEvent<State> {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"ResponseStreamEvent",
			Fields::Named(CONNECTION_EVENT_FIELDS),
		)
	}
}

impl<State: Clone + Send + Sync + 'static> Valuable for ResponseStreamEvent<State> {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			CONNECTION_EVENT_FIELDS,
			&[
				Valuable::as_value(&format!("{}", self.source_address)),
				Valuable::as_value(&self.stream_id),
			],
		));
	}
}

/// The underlying type we use for storing your on connection handler.
pub type UnderlyingOnStreamBeginService<State> =
	BoxCloneService<ResponseStreamEvent<State>, bool, CatBridgeError>;
/// The underlying type we use for storing your on disconnect handler.
pub type UnderlyingOnStreamEndService<State> =
	BoxCloneService<ResponseStreamEvent<State>, (), CatBridgeError>;

/// Extract any value from a Connection event.
///
/// Mirrors [`crate::net::models::FromRequest`].
pub trait FromResponseStreamEvent<State: Clone + Send + Sync + 'static>: Sized {
	fn from_stream_event(
		event: &mut ResponseStreamEvent<State>,
	) -> impl Future<Output = Result<Self, CatBridgeError>> + Send;
}

/// A type that holds onto a [`Service`], and can call it on drop.
///
/// This spawns a temporary task to run the async processing, and moves the
/// event data within.
pub(crate) struct DisconnectAsyncDropServer<
	ServiceTy: Clone
		+ Service<
			ResponseStreamEvent<State>,
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
			ResponseStreamEvent<State>,
			Future = ServiceFutureTy,
			Response = (),
			Error = CatBridgeError,
		> + Send
		+ 'static,
	ServiceFutureTy: Future<Output = Result<(), CatBridgeError>> + Send,
	State: Clone + Send + Sync + 'static,
> DisconnectAsyncDropServer<ServiceTy, ServiceFutureTy, State>
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
			ResponseStreamEvent<State>,
			Future = ServiceFutureTy,
			Response = (),
			Error = CatBridgeError,
		> + Send
		+ 'static,
	ServiceFutureTy: Future<Output = Result<(), CatBridgeError>> + Send,
	State: Clone + Send + Sync + 'static,
> Drop for DisconnectAsyncDropServer<ServiceTy, ServiceFutureTy, State>
{
	fn drop(&mut self) {
		let addr = self.source_address;
		let mut svc = self.service.clone();
		let state = self.state.clone();
		let stream_id = self.stream_id;

		if let Err(cause) = TaskBuilder::new().name("cat_dev::net::server::models::DisconnectAsyncDrop").spawn(async move {
			if let Err(cause) = svc.call(
				ResponseStreamEvent::new_disconnected_with_state(addr, Some(stream_id), state),
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
