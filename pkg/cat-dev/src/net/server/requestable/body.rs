//! Used for extracting the body from any particular request.

use crate::{
	errors::CatBridgeError,
	net::models::{FromRequest, Request},
};
use bytes::Bytes;
use miette::Report;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// Extract the body from a request.
///
/// Consumes the request, and must come last.
pub struct Body<BodyTy>(pub BodyTy);

impl<BodyTy, ErrTy, State: Clone + Send + Sync + 'static> FromRequest<State> for Body<BodyTy>
where
	BodyTy: TryFrom<Bytes, Error = ErrTy>,
	ErrTy: Into<Report> + Send + Sync + 'static,
{
	async fn from_request(req: Request<State>) -> Result<Self, CatBridgeError> {
		BodyTy::try_from(req.body_owned())
			.map(Body)
			.map_err(Into::into)
			.map_err(CatBridgeError::UnknownError)
	}
}

impl<BodyTy> Debug for Body<BodyTy>
where
	BodyTy: Debug,
{
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		fmt.debug_struct("Body").field("inner", &self.0).finish()
	}
}

const BODY_FIELDS: &[NamedField<'static>] = &[NamedField::new("inner")];

impl<BodyTy> Structable for Body<BodyTy>
where
	BodyTy: Valuable,
{
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static("Body", Fields::Named(BODY_FIELDS))
	}
}

impl<BodyTy> Valuable for Body<BodyTy>
where
	BodyTy: Valuable,
{
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(BODY_FIELDS, &[self.0.as_value()]));
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use crate::net::server::router::{Router, test_helpers::*};

	#[tokio::test]
	pub async fn test_extract_body() {
		struct CustomTy(pub Vec<u8>);
		impl CustomTy {
			#[must_use]
			pub fn to_added_bytes(self) -> Vec<u8> {
				self.0.into_iter().map(|num| num + 1).collect::<Vec<_>>()
			}
		}
		impl TryFrom<Bytes> for CustomTy {
			type Error = CatBridgeError;

			fn try_from(body: Bytes) -> Result<Self, Self::Error> {
				let as_vec = Vec::<u8>::from(body);

				assert_eq!(
					as_vec,
					vec![0x1, 0x2, 0x3],
					"Body for custom type was not correct!"
				);

				Ok(Self(as_vec))
			}
		}

		async fn echo_custom(Body(echo): Body<CustomTy>) -> Vec<u8> {
			echo.to_added_bytes()
		}
		let mut router = Router::new();
		router
			.add_route(&[0x1], echo_custom)
			.expect("Failed to add route!");
		assert_eq!(
			router_body_no_close(&mut router, &[0x1, 0x2, 0x3]).await,
			&[0x2, 0x3, 0x4],
		);
	}
}
