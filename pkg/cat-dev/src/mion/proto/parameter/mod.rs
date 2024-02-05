//! Protocols for speaking with the "parameter" port of the MION.
//!
//! The parameter port as of today is only known for getting/setting 512 bytes
//! of parameters. These values are usually things reachable from other parts
//! of the MION interface, but are available through the MION too.

pub mod well_known;

use crate::{
	errors::{APIError, NetworkError, NetworkParseError},
	mion::proto::parameter::well_known::{index_from_parameter_name, ValuableParameterDump},
};
use bytes::{BufMut, Bytes, BytesMut};
use std::fmt::{Display, Formatter, Result as FmtResult};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value};

/// The type of MION Parameters Packet this is.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Valuable)]
pub enum PacketType {
	/// This packet is a read request/response.
	Read,
	/// This packet is a write request/response.
	Write,
}

impl From<&PacketType> for i32 {
	fn from(value: &PacketType) -> Self {
		match *value {
			PacketType::Read => 0,
			PacketType::Write => 1,
		}
	}
}
impl From<PacketType> for i32 {
	fn from(value: PacketType) -> Self {
		Self::from(&value)
	}
}

impl TryFrom<i32> for PacketType {
	type Error = NetworkParseError;

	fn try_from(value: i32) -> Result<Self, Self::Error> {
		match value {
			0 => Ok(Self::Read),
			1 => Ok(Self::Write),
			_ => Err(NetworkParseError::UnknownParamsPacketType(value)),
		}
	}
}

/// A request to dump all the 512 parameters available on the MION board.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Valuable)]
pub struct MionDumpParameters;

impl MionDumpParameters {
	/// Create a new request packet to send to a mion parameter port to tell it
	/// to give us all the parameters.
	#[must_use]
	pub const fn new() -> Self {
		Self {}
	}
}

impl Default for MionDumpParameters {
	fn default() -> Self {
		Self::new()
	}
}

impl Display for MionDumpParameters {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		write!(fmt, "MionDumpParameters")
	}
}

impl TryFrom<Bytes> for MionDumpParameters {
	type Error = NetworkError;

	fn try_from(packet: Bytes) -> Result<Self, Self::Error> {
		if packet.len() < 8 {
			return Err(NetworkError::ParseError(NetworkParseError::NotEnoughData(
				"MionDumpParameters",
				8,
				packet.len(),
				packet,
			)));
		}
		if packet.len() > 8 {
			return Err(NetworkError::ParseError(
				NetworkParseError::UnexpectedTrailer("MionDumpParameters", packet.slice(8..)),
			));
		}

		// Header only -- read request id is 0, so 4 zeroes, and no body afterwards
		// and no error status so 4 0's for length after.
		let static_bytes: &'static [u8] = &[0_u8, 0, 0, 0, 0, 0, 0, 0];
		if packet != static_bytes {
			return Err(NetworkError::ParseError(
				NetworkParseError::PacketDoesntMatchStaticPayload(
					"MionDumpParameters",
					static_bytes,
					packet,
				),
			));
		}

		Ok(Self)
	}
}

impl From<&MionDumpParameters> for Bytes {
	fn from(_: &MionDumpParameters) -> Self {
		BytesMut::zeroed(8).freeze()
	}
}
impl From<MionDumpParameters> for Bytes {
	fn from(value: MionDumpParameters) -> Self {
		Self::from(&value)
	}
}

const DUMPED_MION_PARAMETERS_FIELDS: &[NamedField<'static>] = &[NamedField::new("parameters")];
/// The response from a MION documenting all the parameters available on this board.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct DumpedMionParameters {
	/// All of the parameters available on this board.
	parameters: Bytes,
}

impl DumpedMionParameters {
	/// Get the entire set of parameters for you to mess around with.
	#[must_use]
	pub const fn get_raw_parameters(&self) -> &Bytes {
		&self.parameters
	}

	/// Get a parameter by a name.
	///
	/// ## Errors
	///
	/// - If the name of this parameter is not known.
	pub fn get_parameter_by_name(&self, name: &str) -> Result<u8, APIError> {
		index_from_parameter_name(name)
			.map(|index| self.parameters[index])
			.ok_or_else(|| APIError::MIONParameterNameNotKnown(name.to_owned()))
	}

	/// Get a parameter by a particular index.
	///
	/// ## Errors
	///
	/// - If the index is not within the range of valid parameters.
	pub fn get_parameter_by_index(&self, index: usize) -> Result<u8, APIError> {
		if index > 511 {
			return Err(APIError::MIONParameterNotInRage(index));
		}
		Ok(self.parameters[index])
	}
}

impl TryFrom<Bytes> for DumpedMionParameters {
	type Error = NetworkError;

	fn try_from(packet: Bytes) -> Result<Self, Self::Error> {
		if packet.len() < 520 {
			return Err(NetworkError::ParseError(NetworkParseError::NotEnoughData(
				"DumpedMionParameters",
				520,
				packet.len(),
				packet,
			)));
		}
		if packet.len() > 520 {
			return Err(NetworkError::ParseError(
				NetworkParseError::UnexpectedTrailer("DumpedMionParameters", packet.slice(520..)),
			));
		}

		let header = packet.slice(..8);
		let packet_type = PacketType::try_from(i32::from_le_bytes([
			header[0], header[1], header[2], header[3],
		]))?;
		if packet_type != PacketType::Read {
			return Err(NetworkError::ParseError(
				NetworkParseError::UnknownParamsPacketType(i32::from(packet_type)),
			));
		}
		let size_or_error = i32::from_le_bytes([header[4], header[5], header[6], header[7]]);
		if size_or_error != 512 {
			return Err(NetworkError::ParseError(
				NetworkParseError::ParamsPacketErrorCode(size_or_error),
			));
		}
		let parameters = packet.slice(8..);

		Ok(Self { parameters })
	}
}

impl From<&DumpedMionParameters> for Bytes {
	fn from(value: &DumpedMionParameters) -> Self {
		let mut buff = BytesMut::with_capacity(520);
		buff.put_i32_le(i32::from(PacketType::Read));
		// The size of parameters.
		buff.put_i32_le(512);
		buff.extend_from_slice(&value.parameters);
		buff.freeze()
	}
}
impl From<DumpedMionParameters> for Bytes {
	fn from(value: DumpedMionParameters) -> Self {
		Self::from(&value)
	}
}

impl Structable for DumpedMionParameters {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"DumpedMionParameters",
			Fields::Named(DUMPED_MION_PARAMETERS_FIELDS),
		)
	}
}
impl Valuable for DumpedMionParameters {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn valuable::Visit) {
		let dump = ValuableParameterDump(&self.parameters);

		visitor.visit_named_fields(&NamedValues::new(
			DUMPED_MION_PARAMETERS_FIELDS,
			&[Valuable::as_value(&dump)],
		));
	}
}

/// Set all of the parameters on a MION device.
///
/// There is no way to simply set one byte, you have to set them all at once.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct SetMionParameters {
	parameters: Bytes,
}

impl SetMionParameters {
	/// Create a new packet given a previously dumped header, and the new
	/// complete set of parameters.
	///
	/// This will take a valid `dumped_header`, and will modify it into a
	/// header for setting mion parameters.
	///
	/// ## Errors
	///
	/// - If the `parameters` argument is not exactly 512 bytes long.
	pub fn new(parameters: Bytes) -> Result<Self, APIError> {
		if parameters.len() != 512 {
			return Err(APIError::MIONParameterBodyNotCorrectLength(
				parameters.len(),
			));
		}

		Ok(Self { parameters })
	}

	/// Get the raw parameter body, which contains the raw list of bytes.
	#[must_use]
	pub const fn get_raw_parameters(&self) -> &Bytes {
		&self.parameters
	}

	/// Get a parameter by a name.
	///
	/// ## Errors
	///
	/// - If the name of this parameter is not known.
	pub fn get_parameter_by_name(&self, name: &str) -> Result<u8, APIError> {
		index_from_parameter_name(name)
			.map(|index| self.parameters[index])
			.ok_or_else(|| APIError::MIONParameterNameNotKnown(name.to_owned()))
	}

	/// Get a parameter by a particular index.
	///
	/// ## Errors
	///
	/// - If the index is not within the range of valid parameters.
	pub fn get_parameter_by_index(&self, index: usize) -> Result<u8, APIError> {
		if index > 511 {
			return Err(APIError::MIONParameterNotInRage(index));
		}
		Ok(self.parameters[index])
	}
}

impl TryFrom<Bytes> for SetMionParameters {
	type Error = NetworkError;

	fn try_from(packet: Bytes) -> Result<Self, Self::Error> {
		if packet.len() < 520 {
			return Err(NetworkError::ParseError(NetworkParseError::NotEnoughData(
				"SetMionParameters",
				520,
				packet.len(),
				packet,
			)));
		}
		if packet.len() > 520 {
			return Err(NetworkError::ParseError(
				NetworkParseError::UnexpectedTrailer("SetMionParameters", packet.slice(520..)),
			));
		}

		let header = packet.slice(..8);
		let packet_type = PacketType::try_from(i32::from_le_bytes([
			header[0], header[1], header[2], header[3],
		]))?;
		if packet_type != PacketType::Write {
			return Err(NetworkError::ParseError(
				NetworkParseError::UnknownParamsPacketType(i32::from(packet_type)),
			));
		}
		let size_or_error_code = i32::from_le_bytes([header[4], header[5], header[6], header[7]]);
		if size_or_error_code != 512 {
			return Err(NetworkError::ParseError(
				NetworkParseError::ParamsPacketErrorCode(size_or_error_code),
			));
		}
		let parameters = packet.slice(8..);

		Ok(Self { parameters })
	}
}

impl From<&SetMionParameters> for Bytes {
	fn from(value: &SetMionParameters) -> Self {
		let mut buff = BytesMut::with_capacity(520);
		buff.put_i32_le(i32::from(PacketType::Write));
		// The size of the body.
		buff.put_i32_le(512);
		buff.extend_from_slice(&value.parameters);
		buff.freeze()
	}
}
impl From<SetMionParameters> for Bytes {
	fn from(value: SetMionParameters) -> Self {
		Self::from(&value)
	}
}

/// The response to a [`SetMionParameters`] being sent.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct SetMionParametersResponse {
	/// The return code, anything that isn't a 0 should be considered an error.
	return_code: i32,
}

impl SetMionParametersResponse {
	/// Get the return code of this request.
	///
	/// 0 is used to indicate success, anything else is considered a failure.
	#[must_use]
	pub const fn get_return_code(&self) -> i32 {
		self.return_code
	}

	/// If this operation can be considered a success.
	#[must_use]
	pub const fn is_success(&self) -> bool {
		self.return_code == 0
	}

	/// If this operation error'd out, and was not successful.
	#[must_use]
	pub const fn is_error(&self) -> bool {
		self.return_code != 0
	}
}

impl TryFrom<Bytes> for SetMionParametersResponse {
	type Error = NetworkError;

	fn try_from(packet: Bytes) -> Result<Self, Self::Error> {
		if packet.len() < 12 {
			return Err(NetworkError::ParseError(NetworkParseError::NotEnoughData(
				"SetMionParametersResponse",
				12,
				packet.len(),
				packet,
			)));
		}
		if packet.len() > 12 {
			return Err(NetworkError::ParseError(
				NetworkParseError::UnexpectedTrailer(
					"SetMionParametersResponse",
					packet.slice(12..),
				),
			));
		}

		let header = packet.slice(..8);
		let packet_type = PacketType::try_from(i32::from_le_bytes([
			header[0], header[1], header[2], header[3],
		]))?;
		if packet_type != PacketType::Write {
			return Err(NetworkError::ParseError(
				NetworkParseError::UnknownParamsPacketType(i32::from(packet_type)),
			));
		}
		let size_or_status = i32::from_le_bytes([header[4], header[5], header[6], header[7]]);
		if size_or_status != 4 {
			return Err(NetworkError::ParseError(
				NetworkParseError::ParamsPacketErrorCode(size_or_status),
			));
		}

		let body = packet.slice(8..);
		let return_code = i32::from_le_bytes([body[0], body[1], body[2], body[3]]);

		Ok(Self { return_code })
	}
}

impl From<&SetMionParametersResponse> for Bytes {
	fn from(value: &SetMionParametersResponse) -> Self {
		let mut buff = BytesMut::with_capacity(12);
		buff.put_i32_le(i32::from(PacketType::Write));
		// Size of body.
		buff.put_i32_le(4);
		buff.put_i32_le(value.return_code);
		buff.freeze()
	}
}
impl From<SetMionParametersResponse> for Bytes {
	fn from(value: SetMionParametersResponse) -> Self {
		Self::from(&value)
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use bytes::Bytes;
	use once_cell::sync::Lazy;

	static REAL_LIFE_DUMPED_MION_PARAMETERS_PACKET: Lazy<Vec<u8>> = Lazy::new(|| {
		vec![
			0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x02, 0x02, 0x0c, 0x0d,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff,
		]
	});
	static REAL_LIFE_SET_MION_PARAMETERS_PACKET: Lazy<Vec<u8>> = Lazy::new(|| {
		vec![
			0x01, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x02, 0x02, 0x0c, 0x0d,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
			0xff, 0x45,
		]
	});

	#[test]
	pub fn deser_mion_dump_parameters() {
		// Round-trip success.
		{
			assert!(
				MionDumpParameters::try_from(Bytes::from(MionDumpParameters)).is_ok(),
				"Deserializing a serialized MionDumpParameters was not a success!",
			);
		}

		// Size Differences.
		{
			let short_data = vec![0x0; 4];
			let too_much_data = vec![0x0; 16];

			match MionDumpParameters::try_from(Bytes::from(short_data.clone())) {
				Err(NetworkError::ParseError(parse_val)) => {
					assert_eq!(
						parse_val,
						NetworkParseError::NotEnoughData(
							"MionDumpParameters",
							8,
							short_data.len(),
							Bytes::from(short_data),
						),
					);
				}
				val => panic!("MionDumpParameters parsing too short of data was successful or not a parse error:\n\n {val:?}"),
			}

			match MionDumpParameters::try_from(Bytes::from(too_much_data.clone())) {
				Err(NetworkError::ParseError(parse_val)) => {
					assert_eq!(
						parse_val,
						NetworkParseError::UnexpectedTrailer(
							"MionDumpParameters",
							Bytes::from(too_much_data).slice(8..),
						)
					);
				}
				val => panic!("MionDumpParameters parsing too long of data was successful or not a parse error:\n\n {val:?}"),
			}
		}

		// Invalid packet contents
		{
			let invalid_static_packet = vec![0x0, 0x0, 0x0, 0x1, 0x0, 0x0, 0x0, 0x0];
			match MionDumpParameters::try_from(Bytes::from(invalid_static_packet.clone())) {
				Err(NetworkError::ParseError(parse_val)) => {
					assert_eq!(
						parse_val,
						NetworkParseError::PacketDoesntMatchStaticPayload(
							"MionDumpParameters",
							&[0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0],
							Bytes::from(invalid_static_packet),
						)
					);
				}
				val => panic!("MionDumpParameters parsing too long of data was successful or not a parse error:\n\n {val:?}"),
			}
		}
	}

	#[test]
	pub fn deser_dumped_mion_parameters() {
		// Size Differences
		{
			let short_data = vec![0x0; 519];
			let too_long_data = vec![0x0; 521];

			match DumpedMionParameters::try_from(Bytes::from(short_data.clone())) {
				Err(NetworkError::ParseError(parse_val)) => {
					assert_eq!(
						parse_val,
						NetworkParseError::NotEnoughData(
							"DumpedMionParameters",
							520,
							short_data.len(),
							Bytes::from(short_data),
						),
					);
				}
				val => panic!("DumpedMionParameters parsing too short of data was successful or not a parse error:\n\n {val:?}"),
			}

			match DumpedMionParameters::try_from(Bytes::from(too_long_data.clone())) {
				Err(NetworkError::ParseError(parse_val)) => {
					assert_eq!(
						parse_val,
						NetworkParseError::UnexpectedTrailer(
							"DumpedMionParameters",
							Bytes::from(too_long_data).slice(520..),
						),
					);
				}
				val => panic!("DumpedMionParameters parsing too long of data was successful or not a parse error:\n\n {val:?}"),
			}
		}

		// Invalid Command
		{
			// Bogus value.
			let mut data = vec![0x0; 520];
			data[0] = 11;
			let packet_with_bad_data = Bytes::from(data);

			match DumpedMionParameters::try_from(packet_with_bad_data) {
				Err(NetworkError::ParseError(parse_val)) => {
					assert_eq!(
						parse_val,
						NetworkParseError::UnknownParamsPacketType(11),
					);
				}
				val => panic!("DumpedMionParameters parsing packet with bad packettype was successful or not a parse error:\n\n {val:?}"),
			}

			// Write request, isn't valid either.
			let mut data = vec![0x0; 520];
			data[0] = 1;
			let packet_with_bad_data = Bytes::from(data);

			match DumpedMionParameters::try_from(packet_with_bad_data) {
				Err(NetworkError::ParseError(parse_val)) => {
					assert_eq!(
						parse_val,
						NetworkParseError::UnknownParamsPacketType(1),
					);
				}
				val => panic!("DumpedMionParameters parsing packet with incorrect packettype was successful or not a parse error:\n\n {val:?}"),
			}
		}

		// Real life packet
		{
			let result = DumpedMionParameters::try_from(Bytes::from(
				REAL_LIFE_DUMPED_MION_PARAMETERS_PACKET.clone(),
			));
			assert!(
				result.is_ok(),
				"Failed to parse a real life DumpedMionParameters packet:\n\n  {result:?}",
			);
			let result_two = DumpedMionParameters::try_from(Bytes::from(result.unwrap()));
			assert!(
				result_two.is_ok(),
				"Failed to round-trip real life DumpedMionParameters:\n\n  {result_two:?}",
			);
		}
	}

	#[test]
	pub fn dumped_mion_parameters_api() {
		let parsed_packet = DumpedMionParameters::try_from(Bytes::from(
			REAL_LIFE_DUMPED_MION_PARAMETERS_PACKET.clone(),
		))
		.expect("Failed to parse real life dumped mion parmaeters packet!");

		assert_eq!(
			parsed_packet.get_raw_parameters(),
			&REAL_LIFE_DUMPED_MION_PARAMETERS_PACKET[8..],
			".get_raw_parameters() for DumpedMionParameters did not return the correct body!",
		);

		assert_eq!(
			parsed_packet.get_parameter_by_name("major-version"),
			Ok(0x02),
		);
		assert_eq!(
			parsed_packet.get_parameter_by_name("minor version"),
			Ok(0x0C),
		);
		assert_eq!(
			// Name also should accept indexes that are just a string.
			parsed_packet.get_parameter_by_name("5"),
			Ok(0x0D),
		);
		assert_eq!(
			parsed_packet.get_parameter_by_name("512"),
			Err(APIError::MIONParameterNameNotKnown("512".to_owned())),
		);

		assert_eq!(parsed_packet.get_parameter_by_index(511), Ok(0xFF));
		assert_eq!(
			// 512 is out of bounds as we start counting at 0.
			parsed_packet.get_parameter_by_index(512),
			Err(APIError::MIONParameterNotInRage(512)),
		);
	}

	#[test]
	pub fn deser_set_mion_parameters() {
		// Size Differences
		{
			let short_data = vec![0x0; 519];
			let too_long_data = vec![0x0; 521];

			match SetMionParameters::try_from(Bytes::from(short_data.clone())) {
				Err(NetworkError::ParseError(parse_val)) => {
					assert_eq!(
						parse_val,
						NetworkParseError::NotEnoughData(
							"SetMionParameters",
							520,
							short_data.len(),
							Bytes::from(short_data),
						),
					);
				}
				val => panic!("SetMionParameters parsing too short of data was successful or not a parse error:\n\n {val:?}"),
			}

			match SetMionParameters::try_from(Bytes::from(too_long_data.clone())) {
				Err(NetworkError::ParseError(parse_val)) => {
					assert_eq!(
						parse_val,
						NetworkParseError::UnexpectedTrailer(
							"SetMionParameters",
							Bytes::from(too_long_data).slice(520..),
						),
					);
				}
				val => panic!("SetMionParameters parsing too long of data was successful or not a parse error:\n\n {val:?}"),
			}
		}

		// Invalid Command
		{
			// Bogus value.
			let mut data = vec![0x0; 520];
			data[0] = 11;
			let packet_with_bad_data = Bytes::from(data);

			match SetMionParameters::try_from(packet_with_bad_data) {
				Err(NetworkError::ParseError(parse_val)) => {
					assert_eq!(
						parse_val,
						NetworkParseError::UnknownParamsPacketType(11),
					);
				}
				val => panic!("SetMionParameters parsing packet with bad packettype was successful or not a parse error:\n\n {val:?}"),
			}

			// Write request, isn't valid either.
			let mut data = vec![0x0; 520];
			data[0] = 0;
			let packet_with_bad_data = Bytes::from(data);

			match SetMionParameters::try_from(packet_with_bad_data) {
				Err(NetworkError::ParseError(parse_val)) => {
					assert_eq!(
						parse_val,
						NetworkParseError::UnknownParamsPacketType(0),
					);
				}
				val => panic!("SetMionParameters parsing packet with incorrect packettype was successful or not a parse error:\n\n {val:?}"),
			}
		}

		// Real life packet
		{
			let result = SetMionParameters::try_from(Bytes::from(
				REAL_LIFE_SET_MION_PARAMETERS_PACKET.clone(),
			));
			assert!(
				result.is_ok(),
				"Failed to parse a real life SetMionParameters packet:\n\n  {result:?}",
			);
			let result_two = SetMionParameters::try_from(Bytes::from(result.unwrap()));
			assert!(
				result_two.is_ok(),
				"Failed to round-trip real life SetMionParameters:\n\n  {result_two:?}",
			);
		}
	}

	#[test]
	pub fn set_mion_parameters_api() {
		// Test creation APIs.
		{
			// We should always construct from a dumped packet, because we don't yet
			// know what the header values are, and they could be important :)
			assert!(
				SetMionParameters::new(Bytes::from(&REAL_LIFE_DUMPED_MION_PARAMETERS_PACKET[8..]))
					.is_ok(),
				"Failed to construct `SetMionParameters` from a valid dumped parameters set!",
			);

			// Invalid parameters length
			assert_eq!(
				SetMionParameters::new(Bytes::from(&REAL_LIFE_DUMPED_MION_PARAMETERS_PACKET[7..])),
				Err(APIError::MIONParameterBodyNotCorrectLength(513)),
			);
		}

		// Test non-creation APIs.
		{
			let parsed_packet = SetMionParameters::try_from(Bytes::from(
				REAL_LIFE_SET_MION_PARAMETERS_PACKET.clone(),
			))
			.expect("Failed to parse real life set mion parameters packet!");

			assert_eq!(
				parsed_packet.get_raw_parameters(),
				&REAL_LIFE_SET_MION_PARAMETERS_PACKET[8..],
				".get_raw_parameters() for SetMionParameters did not return the correct body!",
			);

			assert_eq!(
				parsed_packet.get_parameter_by_name("major-version"),
				Ok(0x02),
			);
			assert_eq!(
				parsed_packet.get_parameter_by_name("minor version"),
				Ok(0x0C),
			);
			assert_eq!(
				// Name also should accept indexes that are just a string.
				parsed_packet.get_parameter_by_name("5"),
				Ok(0x0D),
			);
			assert_eq!(
				parsed_packet.get_parameter_by_name("512"),
				Err(APIError::MIONParameterNameNotKnown("512".to_owned())),
			);

			assert_eq!(parsed_packet.get_parameter_by_index(511), Ok(69));
			assert_eq!(
				// 512 is out of bounds as we start counting at 0.
				parsed_packet.get_parameter_by_index(512),
				Err(APIError::MIONParameterNotInRage(512)),
			);
		}
	}

	#[test]
	pub fn deser_set_mion_parameters_response() {
		// Size Mismatch
		{
			let short_data = vec![0x0; 11];
			let too_long_data = vec![0x0; 13];

			match SetMionParametersResponse::try_from(Bytes::from(short_data.clone())) {
				Err(NetworkError::ParseError(parse_val)) => {
					assert_eq!(
						parse_val,
						NetworkParseError::NotEnoughData(
							"SetMionParametersResponse",
							12,
							short_data.len(),
							Bytes::from(short_data),
						),
					);
				}
				val => panic!("SetMionParametersResponse parsing too short of data was successful or not a parse error:\n\n {val:?}"),
			}

			match SetMionParametersResponse::try_from(Bytes::from(too_long_data.clone())) {
				Err(NetworkError::ParseError(parse_val)) => {
					assert_eq!(
						parse_val,
						NetworkParseError::UnexpectedTrailer(
							"SetMionParametersResponse",
							Bytes::from(too_long_data).slice(12..),
						),
					);
				}
				val => panic!("SetMionParametersResponse parsing too long of data was successful or not a parse error:\n\n {val:?}"),
			}
		}

		// unknown packet type
		{
			// Bogus Packet Type
			match SetMionParametersResponse::try_from(Bytes::from(vec![
				// Packet type -- bogus
				0x11, 0x0, 0x0, 0x0,
				// Body length.
				0x4, 0x0, 0x0, 0x0,
				// Return Code
				0x0, 0x0, 0x0, 0x0,
			])) {
				Err(NetworkError::ParseError(parse_val)) => {
					assert_eq!(
						parse_val,
						NetworkParseError::UnknownParamsPacketType(0x11),
					);
				}
				val => panic!("SetMionParametersResponse parsing bogus packet type did not error correctly:\n\n {val:?}"),
			}

			// Wrong Packet Type -- read instead of write
			match SetMionParametersResponse::try_from(Bytes::from(vec![
				// Packet type -- read
				0x0, 0x0, 0x0, 0x0,
				// Body length.
				0x4, 0x0, 0x0, 0x0,
				// Return Code
				0x0, 0x0, 0x0, 0x0,
			])) {
				Err(NetworkError::ParseError(parse_val)) => {
					assert_eq!(
						parse_val,
						NetworkParseError::UnknownParamsPacketType(0),
					);
				}
				val => panic!("SetMionParametersResponse parsing bogus packet type did not error correctly:\n\n {val:?}"),
			}
		}

		// Bad Size/Status
		{
			match SetMionParametersResponse::try_from(Bytes::from(vec![
				// Packet type -- write
				0x1, 0x0, 0x0, 0x0,
				// bogus size.
				0x5, 0x0, 0x0, 0x0,
				// return code
				0x0, 0x0, 0x0, 0x0
			])) {
				Err(NetworkError::ParseError(parse_val)) => {
					assert_eq!(
						parse_val,
						NetworkParseError::ParamsPacketErrorCode(5),
					);
				}
				val => panic!("SetMionParametersResponse parsing bogus size/error_code did not error correctly:\n\n {val:?}"),
			}
		}

		// Successful -- includes roundtrip
		{
			let result = SetMionParametersResponse::try_from(Bytes::from(vec![
				// Packet type -- write
				0x1, 0x0, 0x0, 0x0, // Size -- correct.
				0x4, 0x0, 0x0, 0x0, // Return code -- success!
				0x0, 0x0, 0x0, 0x0,
			]));
			assert!(
				result.is_ok(),
				"Failed to respond to real life SetMionParametersResponse success!"
			);
			assert!(
				SetMionParametersResponse::try_from(Bytes::from(result.unwrap())).is_ok(),
				"Failed to round-trip real-life SetMionParametersResponse success!",
			);
		}
	}
}
