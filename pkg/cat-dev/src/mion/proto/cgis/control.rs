use crate::errors::APIError;
use std::fmt::{Display, Formatter, Result as FmtResult};

/// The type of operations you can do on the `control.cgi` page.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub enum ControlOperation {
	PowerOn,
	PowerOnV2,
	GetInfo,
	SetParam,
}

impl Display for ControlOperation {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		write!(fmt, "{}", Into::<&str>::into(self))
	}
}

impl From<&ControlOperation> for &str {
	fn from(value: &ControlOperation) -> Self {
		match *value {
			ControlOperation::PowerOn => "power_on",
			ControlOperation::PowerOnV2 => "power_on_v2",
			ControlOperation::GetInfo => "get_info",
			ControlOperation::SetParam => "set_param",
		}
	}
}
impl From<ControlOperation> for &str {
	fn from(value: ControlOperation) -> Self {
		Self::from(&value)
	}
}
impl TryFrom<&str> for ControlOperation {
	// This type is an API Error, because we don't ever deserialize it from the
	// network.
	type Error = APIError;

	fn try_from(value: &str) -> Result<Self, Self::Error> {
		match value {
			"power_on" => Ok(Self::PowerOn),
			"power_on_v2" => Ok(Self::PowerOnV2),
			"get_info" => Ok(Self::GetInfo),
			"set_param" => Ok(Self::SetParam),
			val => Err(APIError::UnknownControlOperation(val.to_owned())),
		}
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[test]
	pub fn round_trip_control_operation() {
		for operation in vec![
			ControlOperation::PowerOn,
			ControlOperation::PowerOnV2,
			ControlOperation::GetInfo,
			ControlOperation::SetParam,
		] {
			assert_eq!(
				ControlOperation::try_from(Into::<&str>::into(&operation)),
				Ok(operation),
				"Round-trip conversion of: [{operation}] was not successful!",
			);
		}
	}
}
