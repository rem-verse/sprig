//! Parameters that are well known, and can be referred to by their name
//! rather than just their index.

use crate::errors::APIError;
use bytes::Bytes;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// Ways to specify what parameter to update.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum ParameterLocationSpecification {
	/// Either a name, or an index encoded as a string.
	NameLike(String),
	/// An actual index between 0, and 511 inclusive.
	Index(u16),
}

impl TryFrom<&str> for ParameterLocationSpecification {
	type Error = APIError;

	fn try_from(value: &str) -> Result<Self, Self::Error> {
		if index_from_parameter_name(value).is_some() {
			Ok(Self::NameLike(value.to_owned()))
		} else {
			Err(APIError::MIONParameterNameNotKnown(value.to_owned()))
		}
	}
}
impl TryFrom<&String> for ParameterLocationSpecification {
	type Error = APIError;

	fn try_from(value: &String) -> Result<Self, Self::Error> {
		Self::try_from(value.as_str())
	}
}
impl TryFrom<String> for ParameterLocationSpecification {
	type Error = APIError;

	fn try_from(value: String) -> Result<Self, Self::Error> {
		Self::try_from(value.as_str())
	}
}

impl TryFrom<u16> for ParameterLocationSpecification {
	type Error = APIError;

	fn try_from(value: u16) -> Result<Self, Self::Error> {
		if value < 512 {
			Ok(Self::Index(value))
		} else {
			Err(APIError::MIONParameterNotInRage(usize::from(value)))
		}
	}
}

/// Attempt to get the index of a marater based on a name
#[must_use]
pub fn index_from_parameter_name(name: &str) -> Option<usize> {
	if let Ok(number) = name.parse::<usize>() {
		if number < 512 {
			return Some(number);
		}
	}

	match name {
		"nand-mode" | "nand_mode" | "nandmode" | "nand mode" => Some(2),
		"sdk-major" | "sdk_major" | "sdk major" | "sdk-major-version" | "sdk_major_version"
		| "sdk major version" | "major-version" | "major_version" | "major version" | "major" => Some(3),
		"sdk-minor" | "sdk_minor" | "sdk minor" | "sdk-minor-version" | "sdk_minor_version"
		| "sdk minor version" | "minor-version" | "minor_version" | "minor version" | "minor" => Some(4),
		"sdk-misc" | "sdk_misc" | "sdk misc" | "sdk-misc-version" | "sdk_misc_version"
		| "sdk misc version" | "misc-version" | "misc_version" | "misc version" | "misc" => Some(5),
		_ => None,
	}
}

const PARAMETER_DUMP_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("NandMode"),
	NamedField::new("SdkMajor"),
	NamedField::new("SdkMinor"),
	NamedField::new("SdkMisc"),
	NamedField::new("UnknownParameters"),
];
const KNOWN_INDEXES: &[usize] = &[2_usize, 3_usize, 4_usize, 5_usize];
pub struct ValuableParameterDump<'value>(pub &'value Bytes);
impl<'value> Structable for ValuableParameterDump<'value> {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"ValuableParameterDump",
			Fields::Named(PARAMETER_DUMP_FIELDS),
		)
	}
}
impl<'value> Valuable for ValuableParameterDump<'value> {
	fn as_value(&self) -> valuable::Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		let mut unknown_params = Vec::with_capacity(self.0.len() - KNOWN_INDEXES.len());
		for (idx, byte) in self.0.iter().enumerate() {
			if KNOWN_INDEXES.contains(&idx) {
				continue;
			}
			unknown_params.push((idx, *byte));
		}

		visitor.visit_named_fields(&NamedValues::new(
			PARAMETER_DUMP_FIELDS,
			&[
				Valuable::as_value(&self.0[KNOWN_INDEXES[0]]),
				Valuable::as_value(&self.0[KNOWN_INDEXES[1]]),
				Valuable::as_value(&self.0[KNOWN_INDEXES[2]]),
				Valuable::as_value(&self.0[KNOWN_INDEXES[3]]),
				Valuable::as_value(&unknown_params),
			],
		));
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use bytes::BytesMut;
	use valuable::Visit;

	#[test]
	pub fn can_map_parameter_name_to_index() {
		for (name, expected_index) in vec![
			("nand-mode", Some(2)),
			("nand_mode", Some(2)),
			("nandmode", Some(2)),
			("nand mode", Some(2)),
			("sdk-major", Some(3)),
			("sdk_major", Some(3)),
			("sdk major", Some(3)),
			("sdk-major-version", Some(3)),
			("sdk_major_version", Some(3)),
			("sdk major version", Some(3)),
			("major-version", Some(3)),
			("major_version", Some(3)),
			("major version", Some(3)),
			("major", Some(3)),
			("sdk-minor", Some(4)),
			("sdk_minor", Some(4)),
			("sdk minor", Some(4)),
			("sdk-minor-version", Some(4)),
			("sdk_minor_version", Some(4)),
			("sdk minor version", Some(4)),
			("minor-version", Some(4)),
			("minor_version", Some(4)),
			("minor version", Some(4)),
			("minor", Some(4)),
			("sdk-misc", Some(5)),
			("sdk_misc", Some(5)),
			("sdk misc", Some(5)),
			("sdk-misc-version", Some(5)),
			("sdk_misc_version", Some(5)),
			("sdk misc version", Some(5)),
			("misc-version", Some(5)),
			("misc_version", Some(5)),
			("misc version", Some(5)),
			("misc", Some(5)),
			("trust me babes", None),
			("mics", None),
		] {
			assert_eq!(
				index_from_parameter_name(name),
				expected_index,
				"Parameter name: {name} expected to map to index: {expected_index:?}, but did not get that!",
			);
		}

		for num in 0..512 {
			let displayed = format!("{num}");
			assert_eq!(
				index_from_parameter_name(&displayed),
				Some(num),
				"Parameter name from index as string should be mapped to self!",
			);
		}

		for num in 512..1024 {
			let displayed = format!("{num}");
			assert_eq!(
				index_from_parameter_name(&displayed),
				None,
				"Invalid Parameter index number should not map to any value!",
			);
		}
	}

	#[test]
	pub fn properly_parses_name_fields() {
		struct AssertableVisitor;
		impl Visit for AssertableVisitor {
			fn visit_named_fields(&mut self, named_values: &NamedValues<'_>) {
				for name in PARAMETER_DUMP_FIELDS {
					assert!(
						named_values.get_by_name(name.name()).is_some(),
						"Parameter visitor did not pass a visit that had a required named field!"
					);
				}
			}

			fn visit_value(&mut self, value: Value<'_>) {
				match value {
					Value::Structable(v) => {
						v.visit(self);
					}
					Value::Enumerable(v) => {
						v.visit(self);
					}
					Value::Listable(v) => {
						v.visit(self);
					}
					Value::Mappable(v) => {
						v.visit(self);
					}
					_ => {}
				}
			}
		}

		let empty_param_set = BytesMut::zeroed(512).freeze();
		let dumpee = ValuableParameterDump(&empty_param_set);
		let mut visitor = AssertableVisitor;
		dumpee.visit(&mut visitor);
	}
}
