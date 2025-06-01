//! Handles fetching app wide state that can be attached to a request.
//!
//! If you need dynamic request-local state, prefer [`crate::net::requestable::Extension`].

use crate::{
	errors::CatBridgeError,
	net::{
		models::{FromRef, FromRequest, FromRequestParts, Request},
		server::models::{FromResponseStreamEvent, ResponseStreamEvent},
	},
};
use std::{
	fmt::{Debug, Formatter, Result as FmtResult},
	ops::{Deref, DerefMut},
};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// A type that has been cloned and extracted out of app state.
pub struct State<Ty: Clone + Send + Sync + 'static>(pub Ty);

impl<
	OuterState: Clone + Send + Sync + 'static,
	InnerState: FromRef<OuterState> + Clone + Send + Sync + 'static,
> FromRequestParts<OuterState> for State<InnerState>
{
	async fn from_request_parts(req: &mut Request<OuterState>) -> Result<Self, CatBridgeError> {
		Ok(Self(InnerState::from_ref(req.state())))
	}
}

impl<
	OuterState: Clone + Send + Sync + 'static,
	InnerState: FromRef<OuterState> + Clone + Send + Sync + 'static,
> FromRequest<OuterState> for State<InnerState>
{
	async fn from_request(req: Request<OuterState>) -> Result<Self, CatBridgeError> {
		Ok(Self(InnerState::from_ref(req.state())))
	}
}

impl<
	OuterState: Clone + Send + Sync + 'static,
	InnerState: FromRef<OuterState> + Clone + Send + Sync + 'static,
> FromResponseStreamEvent<OuterState> for State<InnerState>
{
	async fn from_stream_event(
		evt: &mut ResponseStreamEvent<OuterState>,
	) -> Result<Self, CatBridgeError> {
		Ok(Self(InnerState::from_ref(evt.state())))
	}
}

impl<Ty: Clone + Send + Sync + 'static> Deref for State<Ty> {
	type Target = Ty;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl<Ty: Clone + Send + Sync + 'static> DerefMut for State<Ty> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}

impl<Ty: Clone + Send + Sync + 'static> Debug for State<Ty>
where
	Ty: Debug,
{
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		fmt.debug_struct("State").field("inner", &self.0).finish()
	}
}

const STATE_FIELDS: &[NamedField<'static>] = &[NamedField::new("inner")];

impl<Ty: Clone + Send + Sync + 'static> Structable for State<Ty>
where
	Ty: Valuable,
{
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static("State", Fields::Named(STATE_FIELDS))
	}
}

impl<Ty: Clone + Send + Sync + 'static> Valuable for State<Ty>
where
	Ty: Valuable,
{
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(STATE_FIELDS, &[self.0.as_value()]));
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use crate::net::server::router::{Router, test_helpers::router_body_no_close_with_state};

	#[tokio::test]
	pub async fn test_state() {
		async fn echo_state(State(data): State<String>) -> String {
			data
		}

		let mut router = Router::<String>::new();
		router
			.add_route(&[0x1], echo_state)
			.expect("Failed to add route!");
		assert_eq!(
			router_body_no_close_with_state(
				&mut router,
				&[0x1, 0x2, 0x3, 0x4],
				"Hey from state!".to_owned(),
			)
			.await,
			b"Hey from state!",
		);
	}
}
