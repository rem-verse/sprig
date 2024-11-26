use crate::errors::APIError;
use std::fmt::{Display, Formatter, Result as FmtResult};

/// The type of operations you can do on the `status.cgi` page.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub enum StatusOperation {
	Eject,
}

impl Display for StatusOperation {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		write!(fmt, "{}", Into::<&str>::into(self))
	}
}

impl From<&StatusOperation> for &str {
	fn from(value: &StatusOperation) -> Self {
		match *value {
			StatusOperation::Eject => "eject",
		}
	}
}
impl From<StatusOperation> for &str {
	fn from(value: StatusOperation) -> Self {
		Self::from(&value)
	}
}
impl TryFrom<&str> for StatusOperation {
	// This type is an API Error, because we don't ever deserialize it from the
	// network.
	type Error = APIError;

	fn try_from(value: &str) -> Result<Self, Self::Error> {
		match value {
			"eject" => Ok(Self::Eject),
			val => Err(APIError::UnknownStatusOperation(val.to_owned())),
		}
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[test]
	pub fn round_trip_status_operation() {
		for operation in vec![StatusOperation::Eject] {
			assert_eq!(
				StatusOperation::try_from(Into::<&str>::into(&operation)),
				Ok(operation),
				"Round-trip conversion of: [{operation}] was not successful!",
			);
		}
	}
}
