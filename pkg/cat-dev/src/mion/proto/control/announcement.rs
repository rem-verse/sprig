//! Packets related to announcements, and "finding" bridges.
//!
//! Specifically the packet types:
//!
//! - [`crate::mion::proto::MionCommandByte::AnnounceYourselves`]
//! - [`crate::mion::proto::MionCommandByte::AcknowledgeAnnouncement`]
//!
//! And all the associated structures necessary to parse those packets.

use crate::{
	errors::{APIError, NetworkError, NetworkParseError},
	mion::proto::control::MionCommandByte,
};
use bytes::{BufMut, Bytes, BytesMut};
use mac_address::MacAddress;
use std::{
	fmt::{Display, Formatter, Result as FmtResult},
	net::Ipv4Addr,
};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// The data to pass along in the "Announce Yourself" message.
const ANNOUNCEMENT_MESSAGE: &str = "MULTI_I/O_NETWORK_BOARD";
/// The flag to encode into the packet to request more detailed information.
const DETAIL_FLAG_MESSAGE: &str = "enumV1";

/// An announcement to ask all MION's to identify themselves.
///
/// Provide "detailed" to get more than the IP/Mac/Name/FPGA Version/FW Version
/// fields.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Valuable)]
pub struct MionIdentityAnnouncement {
	/// If we should query for extra information, this corresponds to the
	/// `-detail` flag from `findbridge`, and in the actual protocol maps
	/// to `enumV1`.
	detailed: bool,
}
impl MionIdentityAnnouncement {
	#[must_use]
	pub const fn new(is_detailed: bool) -> Self {
		Self {
			detailed: is_detailed,
		}
	}

	/// If we are going to ask the MIONs to include more information about
	/// themselves.
	#[must_use]
	pub const fn is_detailed(&self) -> bool {
		self.detailed
	}
}
impl Display for MionIdentityAnnouncement {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		write!(
			fmt,
			"{}",
			if self.detailed {
				"DetailedMionIdentityAnnouncement"
			} else {
				"MionIdentityAnnouncement"
			}
		)
	}
}
impl TryFrom<Bytes> for MionIdentityAnnouncement {
	type Error = NetworkError;

	fn try_from(packet: Bytes) -> Result<Self, Self::Error> {
		if packet.len() < 25 {
			return Err(NetworkError::ParseError(NetworkParseError::NotEnoughData(
				"MionIdentityAnnouncement",
				25,
				packet.len(),
				packet,
			)));
		}
		if packet.len() > 33 {
			return Err(NetworkError::ParseError(
				NetworkParseError::UnexpectedTrailer(
					"MionIdentityAnnouncement",
					packet.slice(33..),
				),
			));
		}
		let is_detailed = packet.len() > 25;

		if packet[0] != u8::from(MionCommandByte::AnnounceYourselves) {
			return Err(NetworkError::ParseError(NetworkParseError::UnknownCommand(
				packet[0],
			)));
		}

		if &packet[1..24] != ANNOUNCEMENT_MESSAGE.as_bytes() {
			return Err(NetworkError::ParseError(
				NetworkParseError::FieldEncodedIncorrectly(
					"MionIdentityAnnouncement",
					"buff",
					"Must start with static message: `MULTI_I/O_NETWORK_BOARD` with a NUL Terminator",
				),
			));
		}
		if packet[24] != 0 {
			return Err(NetworkError::ParseError(
				NetworkParseError::FieldEncodedIncorrectly(
					"MionIdentityAnnouncement",
					"buff",
					"Must start with static message: `MULTI_I/O_NETWORK_BOARD` with a NUL Terminator",
				),
			));
		}
		if is_detailed && &packet[25..] != b"enumV1\0\0" {
			return Err(NetworkError::ParseError(
				NetworkParseError::FieldEncodedIncorrectly(
					"MionIdentityAnnouncement",
					"buff",
					"Only the static string `enumV1` followed by two NUL Terminators is allowed after `MULTI_I/O_NETWORK_BOARD`.",
				),
			));
		}

		Ok(Self {
			detailed: is_detailed,
		})
	}
}
impl From<&MionIdentityAnnouncement> for Bytes {
	fn from(this: &MionIdentityAnnouncement) -> Self {
		let mut buff = BytesMut::with_capacity(if this.detailed { 33 } else { 25 });
		buff.put_u8(u8::from(MionCommandByte::AnnounceYourselves));
		buff.extend_from_slice(ANNOUNCEMENT_MESSAGE.as_bytes());
		buff.put_u8(0);
		if this.detailed {
			buff.extend_from_slice(DETAIL_FLAG_MESSAGE.as_bytes());
			buff.put_u16(0_u16);
		}
		buff.freeze()
	}
}
impl From<MionIdentityAnnouncement> for Bytes {
	fn from(value: MionIdentityAnnouncement) -> Self {
		Self::from(&value)
	}
}

/// The boot type the MION is actively configured to boot into.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Valuable)]
pub enum MIONBootType {
	/// Boot from the PC rather than from it's own internal device nand.
	PCFS,
	/// An unknown boot type we don't know how to parse.
	Unk(u8),
}
impl Display for MIONBootType {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		match *self {
			Self::PCFS => write!(fmt, "PCFS"),
			Self::Unk(val) => write!(fmt, "Unk({val})"),
		}
	}
}
impl From<u8> for MIONBootType {
	fn from(value: u8) -> Self {
		match value {
			0x2 => MIONBootType::PCFS,
			num => MIONBootType::Unk(num),
		}
	}
}

/// An identity for a CAT-DEV that we received from the network.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct MionIdentity {
	// All the extra detailed data, most of this is unknown, though some bytes
	// are able to be read.
	detailed_all: Option<Bytes>,
	firmware_version: [u8; 4],
	fpga_version: [u8; 4],
	ip_address: Ipv4Addr,
	mac: MacAddress,
	name: String,
}
impl MionIdentity {
	/// Create a new MION Identity from scratch.
	///
	/// ## Errors
	///
	/// - If the name is not ASCII.
	/// - If the name is longer than 255 bytes.
	/// - If the name is empty.
	pub fn new(
		detailed_data: Option<Bytes>,
		firmware_version: [u8; 4],
		fpga_version: [u8; 4],
		ip_address: Ipv4Addr,
		mac: MacAddress,
		name: String,
	) -> Result<Self, APIError> {
		if !name.is_ascii() {
			return Err(APIError::DeviceNameMustBeAscii);
		}
		if name.len() > 255 {
			return Err(APIError::DeviceNameTooLong(name.len()));
		}
		if name.is_empty() {
			return Err(APIError::DeviceNameCannotBeEmpty);
		}

		Ok(Self {
			detailed_all: detailed_data,
			firmware_version,
			fpga_version,
			ip_address,
			mac,
			name,
		})
	}

	/// The firmware version of the current CAT-DEV, rendered as a string you'd
	/// see displayed.
	#[must_use]
	pub fn firmware_version(&self) -> String {
		format!(
			"0.{}.{}.{}",
			self.firmware_version[0], self.firmware_version[1], self.firmware_version[2],
		)
	}
	/// The firmware version of the current CAT-DEV.
	///
	/// Each part is split into it's own byte. If you want a string
	/// representation call [`MionIdentity::firmware_version`].
	#[must_use]
	pub const fn raw_firmware_version(&self) -> [u8; 4] {
		self.firmware_version
	}

	/// The FPGA of the current CAT-DEV, rendered as a string you'd see
	/// displayed in a list view.
	#[must_use]
	pub fn fpga_version(&self) -> String {
		let mut fpga_version = String::new();
		for byte in [
			self.fpga_version[3],
			self.fpga_version[2],
			self.fpga_version[1],
			self.fpga_version[0],
		] {
			fpga_version.push_str(&format!("{byte:x}"));
		}
		fpga_version
	}
	/// The FPGA of the current CAT-DEV, rendered as a string you'd see
	/// displayed in a detail view.
	#[must_use]
	pub fn detailed_fpga_version(&self) -> String {
		let mut fpga_version = String::new();
		for byte in [
			self.fpga_version[3],
			self.fpga_version[2],
			self.fpga_version[1],
			self.fpga_version[0],
		] {
			fpga_version.push_str(&format!("{byte:02x}"));
		}
		fpga_version
	}
	/// The version of the FPGA on the CAT-DEV.
	///
	/// Each part is split into it's own byte. If you want a string
	/// representation call [`MionIdentity::fpga_version`].
	#[must_use]
	pub const fn raw_fpga_version(&self) -> [u8; 4] {
		self.fpga_version
	}

	/// The IP Address this identity belongs to.
	#[must_use]
	pub const fn ip_address(&self) -> Ipv4Addr {
		self.ip_address
	}

	/// The Mac Address of the identity belongs to.
	#[must_use]
	pub const fn mac_address(&self) -> MacAddress {
		self.mac
	}

	/// The name of this machine.
	#[must_use]
	pub fn name(&self) -> &str {
		&self.name
	}

	/// If the data the client sent back to us was _detailed_, and contains extra
	/// bits of information.
	///
	/// NOTE: for old enough firmwares, even if you ask for detailed data you may
	/// not get it.
	#[must_use]
	pub const fn is_detailed(&self) -> bool {
		self.detailed_all.is_some()
	}

	/// If you've asked for, and received detailed information this will be the
	/// SDK version that the current dev-kit is running.
	#[must_use]
	pub fn detailed_sdk_version(&self) -> Option<String> {
		self.detailed_all.as_ref().map(|extra_data| {
			let bytes = [
				extra_data[227],
				extra_data[228],
				extra_data[229],
				extra_data[230],
			];

			// SDK versions may not display the fourth identifier.
			if bytes[3] == 0 {
				format!("{}.{}.{}", bytes[0], bytes[1], bytes[2])
			} else {
				format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3])
			}
		})
	}
	/// If you've asked for, and received detailed information this will be the
	/// SDK version that the current dev-kit is running.
	///
	/// These are the 4 raw bytes returned from the response. If you want to
	/// display these as a string somewhere you should use the method:
	/// [`MionIdentity::detailed_sdk_version`].
	#[must_use]
	pub fn detailed_raw_sdk_version(&self) -> Option<[u8; 4]> {
		self.detailed_all.as_ref().map(|extra_data| {
			[
				extra_data[227],
				extra_data[228],
				extra_data[229],
				extra_data[230],
			]
		})
	}

	/// If you've asked for, and received detailed information this will be the
	/// boot-type that the device is configured to use.
	#[must_use]
	pub fn detailed_boot_type(&self) -> Option<MIONBootType> {
		self.detailed_all
			.as_ref()
			.map(|extra_data| MIONBootType::from(extra_data[232]))
	}

	/// If you've asked for, and received detailed information this will be the
	/// status of cafe being on/off.
	#[must_use]
	pub fn detailed_is_cafe_on(&self) -> Option<bool> {
		self.detailed_all
			.as_ref()
			.map(|extra_data| extra_data[233] > 0)
	}
}
impl Display for MionIdentity {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		if let Some(detailed) = self.detailed_all.as_ref() {
			write!(
				fmt,
				"{} (aka {}) @ {} fpga-v{}.{}.{}.{} fw-v{}.{}.{}.{} sdk-v{}.{}.{}.{} boot-type:{} cafe:{}",
				self.name,
				self.ip_address,
				self.mac,
				self.fpga_version[0],
				self.fpga_version[1],
				self.fpga_version[2],
				self.fpga_version[3],
				self.firmware_version[0],
				self.firmware_version[1],
				self.firmware_version[2],
				self.firmware_version[3],
				detailed[227],
				detailed[228],
				detailed[229],
				detailed[230],
				MIONBootType::from(detailed[232]),
				detailed[233],
			)
		} else {
			write!(
				fmt,
				"{} (aka {}) @ {} fpga-v{}.{}.{}.{} fw-v{}.{}.{}.{}",
				self.name,
				self.ip_address,
				self.mac,
				self.fpga_version[0],
				self.fpga_version[1],
				self.fpga_version[2],
				self.fpga_version[3],
				self.firmware_version[0],
				self.firmware_version[1],
				self.firmware_version[2],
				self.firmware_version[3],
			)
		}
	}
}
impl TryFrom<(Ipv4Addr, Bytes)> for MionIdentity {
	type Error = NetworkError;

	fn try_from((from_address, packet): (Ipv4Addr, Bytes)) -> Result<Self, Self::Error> {
		// Packet must be at least 18 bytes.
		//
		// Name starts at the 17th byte, and must be at least one byte long.
		if packet.len() < 17 {
			return Err(NetworkError::ParseError(NetworkParseError::NotEnoughData(
				"MionIdentity",
				17,
				packet.len(),
				packet,
			)));
		}

		if packet[0] != u8::from(MionCommandByte::AcknowledgeAnnouncement) {
			return Err(NetworkError::ParseError(NetworkParseError::UnknownCommand(
				packet[0],
			)));
		}
		// Name is variable in size, so we need to make sure we need there is
		// enough space. Name length is stored at index 7, and is just one byte
		// long.
		let name_length = usize::from(packet[7]);
		if packet.len() < 16 + name_length {
			return Err(NetworkError::ParseError(NetworkParseError::NotEnoughData(
				"MionIdentity",
				16 + name_length,
				packet.len(),
				packet,
			)));
		}
		if name_length < 1 {
			return Err(NetworkError::ParseError(
				NetworkParseError::FieldNotLongEnough(
					"MionIdentity",
					"name",
					1,
					name_length,
					packet,
				),
			));
		}
		if packet.len() > 16 + name_length + 239 {
			return Err(NetworkError::ParseError(
				NetworkParseError::UnexpectedTrailer(
					"MionIdentity",
					packet.slice(16 + name_length + 239..),
				),
			));
		}
		if packet.len() != 16 + name_length && packet.len() != 16 + name_length + 239 {
			return Err(NetworkError::ParseError(
				NetworkParseError::UnexpectedTrailer(
					"MionIdentity",
					packet.slice(16 + name_length..),
				),
			));
		}
		let is_detailed = packet.len() > 16 + name_length;

		let mac = MacAddress::new([
			packet[1], packet[2], packet[3], packet[4], packet[5], packet[6],
		]);
		let fpga_version = [packet[8], packet[9], packet[10], packet[11]];
		let firmware_version = [packet[12], packet[13], packet[14], packet[15]];
		let Ok(name) = String::from_utf8(Vec::from(&packet[16..16 + name_length])) else {
			return Err(NetworkError::ParseError(
				NetworkParseError::FieldEncodedIncorrectly("MionIdentity", "name", "ASCII"),
			));
		};
		if !name.is_ascii() {
			return Err(NetworkError::ParseError(
				NetworkParseError::FieldEncodedIncorrectly("MionIdentity", "name", "ASCII"),
			));
		}

		let detailed_all = if is_detailed {
			Some(packet.slice(16 + name_length..))
		} else {
			None
		};

		Ok(Self {
			detailed_all,
			firmware_version,
			fpga_version,
			ip_address: from_address,
			mac,
			name,
		})
	}
}
impl From<&MionIdentity> for Bytes {
	fn from(value: &MionIdentity) -> Self {
		let mut buff = BytesMut::with_capacity(16 + value.name.len());
		buff.put_u8(u8::from(MionCommandByte::AcknowledgeAnnouncement));
		buff.extend_from_slice(&value.mac.bytes());
		buff.put_u8(u8::try_from(value.name.len()).unwrap_or(u8::MAX));
		buff.extend_from_slice(&[
			value.fpga_version[0],
			value.fpga_version[1],
			value.fpga_version[2],
			value.fpga_version[3],
		]);
		buff.extend_from_slice(&[
			value.firmware_version[0],
			value.firmware_version[1],
			value.firmware_version[2],
			value.firmware_version[3],
		]);
		buff.extend_from_slice(value.name.as_bytes());
		buff.freeze()
	}
}
impl From<MionIdentity> for Bytes {
	fn from(value: MionIdentity) -> Self {
		Self::from(&value)
	}
}

const MION_IDENTITY_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("name"),
	NamedField::new("ip_address"),
	NamedField::new("mac"),
	NamedField::new("fpga_version"),
	NamedField::new("firmware_version"),
	NamedField::new("detailed_sdk_version"),
	NamedField::new("detailed_boot_mode"),
	NamedField::new("detailed_power_status"),
];
impl Structable for MionIdentity {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static("MionIdentity", Fields::Named(MION_IDENTITY_FIELDS))
	}
}
impl Valuable for MionIdentity {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		let detailed_sdk_version = self
			.detailed_sdk_version()
			.unwrap_or("<missing data>".to_owned());
		let detailed_boot_mode = self
			.detailed_boot_type()
			.map_or("<missing data>".to_owned(), |bt| format!("{bt}"));
		let detailed_power_status = self
			.detailed_sdk_version()
			.unwrap_or("<missing data>".to_owned());

		visitor.visit_named_fields(&NamedValues::new(
			MION_IDENTITY_FIELDS,
			&[
				Valuable::as_value(&self.name),
				Valuable::as_value(&format!("{}", self.ip_address)),
				Valuable::as_value(&format!("{}", self.mac)),
				Valuable::as_value(&self.detailed_fpga_version()),
				Valuable::as_value(&self.firmware_version()),
				Valuable::as_value(&detailed_sdk_version),
				Valuable::as_value(&detailed_boot_mode),
				Valuable::as_value(&detailed_power_status),
			],
		));
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[test]
	pub fn mion_command_byte_conversions() {
		for command_byte in vec![
			MionCommandByte::Search,
			MionCommandByte::Broadcast,
			MionCommandByte::AnnounceYourselves,
			MionCommandByte::AcknowledgeAnnouncement,
		] {
			assert_eq!(
				MionCommandByte::try_from(u8::from(command_byte))
					.expect("Failed to turn command byte -> u8 -> command byte"),
				command_byte,
				"Mion Command Byte when serialized & deserialized was not the same: {}",
				command_byte,
			);
		}
	}

	#[test]
	pub fn mion_identity_construction_tests() {
		assert_eq!(
			MionIdentity::new(
				None,
				[0, 0, 0, 0],
				[0, 0, 0, 0],
				Ipv4Addr::LOCALHOST,
				MacAddress::new([0, 0, 0, 0, 0, 0]),
				// Doesn't fall within the ASCII range.
				"Ƙ".to_owned()
			),
			Err(APIError::DeviceNameMustBeAscii),
		);
		assert_eq!(
			MionIdentity::new(
				None,
				[0, 0, 0, 0],
				[0, 0, 0, 0],
				Ipv4Addr::LOCALHOST,
				MacAddress::new([0, 0, 0, 0, 0, 0]),
				// Device name cannot be more than 255 bytes.
				"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned()
			),
			Err(APIError::DeviceNameTooLong(300)),
		);
		assert_eq!(
			MionIdentity::new(
				None,
				[0, 0, 0, 0],
				[0, 0, 0, 0],
				Ipv4Addr::LOCALHOST,
				MacAddress::new([0, 0, 0, 0, 0, 0]),
				// Cannot be empty!
				String::new(),
			),
			Err(APIError::DeviceNameCannotBeEmpty),
		);
		// Success!
		assert!(MionIdentity::new(
			None,
			[0, 0, 0, 0],
			[0, 0, 0, 0],
			Ipv4Addr::LOCALHOST,
			MacAddress::new([0, 0, 0, 0, 0, 0]),
			"00-00-00-00-00-00".to_owned(),
		)
		.is_ok());
	}

	#[test]
	pub fn mion_identity_deser() {
		// Successful Serialization & Deserialization.
		{
			let identity = MionIdentity::new(
				None,
				[1, 2, 3, 4],
				[5, 6, 7, 8],
				Ipv4Addr::new(9, 10, 11, 12),
				MacAddress::new([13, 14, 15, 16, 17, 18]),
				"Apples".to_owned(),
			)
			.expect("Failed to create identity to serialize & deserialize.");

			assert_eq!(
				identity,
				MionIdentity::try_from((Ipv4Addr::new(9, 10, 11, 12), Bytes::from(&identity)))
					.expect("Failed to deserialize MION Identity")
			);
		}

		// Too short no actual name field.
		{
			let buff = Bytes::from(vec![
				// Command Byte
				u8::from(MionCommandByte::AcknowledgeAnnouncement),
				// Mac
				0x1,
				0x2,
				0x3,
				0x4,
				0x5,
				0x6,
				// Name length
				0x1,
				// FPGA Version
				0x1,
				0x2,
				0x3,
				0x4,
				// Firmware Version
				0x1,
				0x2,
				0x3,
				0x4,
			]);

			assert!(matches!(
				MionIdentity::try_from((Ipv4Addr::LOCALHOST, buff.clone())),
				Err(NetworkError::ParseError(NetworkParseError::NotEnoughData(
					"MionIdentity",
					17,
					16,
					_,
				))),
			));
		}

		// Command Byte is not correct.
		{
			let buff = Bytes::from(vec![
				// Command Byte
				u8::from(MionCommandByte::Search),
				// Mac
				0x1,
				0x2,
				0x3,
				0x4,
				0x5,
				0x6,
				// Name length
				0x1,
				// FPGA Version
				0x1,
				0x2,
				0x3,
				0x4,
				// Firmware Version
				0x1,
				0x2,
				0x3,
				0x4,
				// Name
				101,
			]);

			let result = MionIdentity::try_from((Ipv4Addr::LOCALHOST, buff.clone()));
			assert!(matches!(result, Err(NetworkError::ParseError(_))));
			// Guaranteed to be taken!
			if let Err(NetworkError::ParseError(value)) = result {
				assert_eq!(value, NetworkParseError::UnknownCommand(0x3F));
			}
		}

		// Name is not long enough.
		{
			let buff = Bytes::from(vec![
				// Command Byte
				u8::from(MionCommandByte::AcknowledgeAnnouncement),
				// Mac
				0x1,
				0x2,
				0x3,
				0x4,
				0x5,
				0x6,
				// Name length -- too short.
				0x0,
				// FPGA Version
				0x1,
				0x2,
				0x3,
				0x4,
				// Firmware Version
				0x1,
				0x2,
				0x3,
				0x4,
				// Name still needs to be present to pass initial length check.
				101,
			]);

			let result = MionIdentity::try_from((Ipv4Addr::LOCALHOST, buff.clone()));
			assert!(matches!(result, Err(NetworkError::ParseError(_))));
			// Guaranteed to be taken!
			if let Err(NetworkError::ParseError(value)) = result {
				assert_eq!(
					value,
					NetworkParseError::FieldNotLongEnough("MionIdentity", "name", 1, 0, buff)
				);
			}
		}

		// Name not UTF-8.
		{
			let buff = Bytes::from(vec![
				// Command Byte
				u8::from(MionCommandByte::AcknowledgeAnnouncement),
				// Mac
				0x1,
				0x2,
				0x3,
				0x4,
				0x5,
				0x6,
				// Name length
				0x6,
				// FPGA Version
				0x1,
				0x2,
				0x3,
				0x4,
				// Firmware Version
				0x1,
				0x2,
				0x3,
				0x4,
				// Name, invalid UTF-8
				0xFF,
				0xFF,
				0xFF,
				0xFF,
				0xFF,
				0xFF,
			]);

			let result = MionIdentity::try_from((Ipv4Addr::LOCALHOST, buff.clone()));
			assert!(matches!(result, Err(NetworkError::ParseError(_))));
			// Guaranteed to be taken!
			if let Err(NetworkError::ParseError(value)) = result {
				assert_eq!(
					value,
					NetworkParseError::FieldEncodedIncorrectly("MionIdentity", "name", "ASCII")
				);
			}
		}

		// Name UTF-8 but not ascii.
		{
			let buff = Bytes::from(vec![
				// Command Byte
				u8::from(MionCommandByte::AcknowledgeAnnouncement),
				// Mac
				0x1,
				0x2,
				0x3,
				0x4,
				0x5,
				0x6,
				// Name length
				0x2,
				// FPGA Version
				0x1,
				0x2,
				0x3,
				0x4,
				// Firmware Version
				0x1,
				0x2,
				0x3,
				0x4,
				// Name, "Ƙ" UTF-8 not ascii.
				0xC6,
				0x98,
			]);

			let result = MionIdentity::try_from((Ipv4Addr::LOCALHOST, buff.clone()));
			assert!(matches!(result, Err(NetworkError::ParseError(_))));
			// Guaranteed to be taken!
			if let Err(NetworkError::ParseError(value)) = result {
				assert_eq!(
					value,
					NetworkParseError::FieldEncodedIncorrectly("MionIdentity", "name", "ASCII")
				);
			}
		}

		// Unexpected trailer that isn't detailed.
		{
			let mut buff = BytesMut::new();
			buff.extend_from_slice(&[
				// Command Byte
				u8::from(MionCommandByte::AcknowledgeAnnouncement),
				// Mac
				0x1,
				0x2,
				0x3,
				0x4,
				0x5,
				0x6,
				// Name length
				0x2,
				// FPGA Version
				0x1,
				0x2,
				0x3,
				0x4,
				// Firmware Version
				0x1,
				0x2,
				0x3,
				0x4,
				// Name.
				0x61,
				0x61,
			]);
			// Unexpected trailers...
			buff.extend_from_slice(b"abcd");

			let result = MionIdentity::try_from((Ipv4Addr::LOCALHOST, buff.freeze()));
			assert!(matches!(result, Err(NetworkError::ParseError(_))));
			// Guaranteed to be taken!
			if let Err(NetworkError::ParseError(value)) = result {
				assert_eq!(
					value,
					NetworkParseError::UnexpectedTrailer(
						"MionIdentity",
						Bytes::from(b"abcd".iter().cloned().collect::<Vec<u8>>())
					)
				);
			}
		}

		// Unexpected trailing data on fully detailed packet.
		{
			let mut buff = BytesMut::new();
			buff.extend_from_slice(&[
				// Command Byte
				u8::from(MionCommandByte::AcknowledgeAnnouncement),
				// Mac
				0x1,
				0x2,
				0x3,
				0x4,
				0x5,
				0x6,
				// Name length
				0x2,
				// FPGA Version
				0x1,
				0x2,
				0x3,
				0x4,
				// Firmware Version
				0x1,
				0x2,
				0x3,
				0x4,
				// Name.
				0x61,
				0x61,
			]);
			// Pad extra detailed data.
			buff.extend_from_slice(&[0x0; 239]);
			// Unexpected trailers...
			buff.extend_from_slice(b"abcd");

			let result = MionIdentity::try_from((Ipv4Addr::LOCALHOST, buff.freeze()));
			assert!(matches!(result, Err(NetworkError::ParseError(_))));
			// Guaranteed to be taken!
			if let Err(NetworkError::ParseError(value)) = result {
				assert_eq!(
					value,
					NetworkParseError::UnexpectedTrailer(
						"MionIdentity",
						Bytes::from(b"abcd".iter().cloned().collect::<Vec<u8>>())
					)
				);
			}
		}
	}

	#[test]
	pub fn test_real_life_detailed_announcements() {
		const OFF_ANNOUNCEMENT: [u8; 272] = [
			0x20, 0x00, 0x25, 0x5c, 0xba, 0x5a, 0x00, 0x11, 0x71, 0x20, 0x05, 0x13, 0x00, 0x0e,
			0x50, 0x01, 0x30, 0x30, 0x2d, 0x32, 0x35, 0x2d, 0x35, 0x43, 0x2d, 0x42, 0x41, 0x2d,
			0x35, 0x41, 0x2d, 0x30, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x0c, 0x0d, 0x00, 0x01, 0x02,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
		];
		const ON_ANNOUNCEMENT: [u8; 272] = [
			0x20, 0x00, 0x25, 0x5c, 0xba, 0x5a, 0x00, 0x11, 0x71, 0x20, 0x05, 0x13, 0x00, 0x0e,
			0x50, 0x01, 0x30, 0x30, 0x2d, 0x32, 0x35, 0x2d, 0x35, 0x43, 0x2d, 0x42, 0x41, 0x2d,
			0x35, 0x41, 0x2d, 0x30, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x0c, 0x0d, 0x00, 0x01, 0x02,
			0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
		];

		let off_identity = MionIdentity::try_from((
			Ipv4Addr::LOCALHOST,
			Bytes::from(Vec::from(OFF_ANNOUNCEMENT)),
		))
		.expect("Failed to parse `OFF_ANNOUNCEMENT` from an actual data packet. Parser is broken.");
		let on_identity =
			MionIdentity::try_from((Ipv4Addr::LOCALHOST, Bytes::from(Vec::from(ON_ANNOUNCEMENT))))
				.expect(
				"Failed to parse `ON_ANNOUNCEMENT` from an actual data packet. Parser is broken.",
			);

		assert_eq!(
			off_identity.detailed_sdk_version(),
			Some("2.12.13".to_owned())
		);
		assert_eq!(off_identity.detailed_boot_type(), Some(MIONBootType::PCFS));
		assert_eq!(off_identity.detailed_is_cafe_on(), Some(false));

		assert_eq!(
			on_identity.detailed_sdk_version(),
			Some("2.12.13".to_owned())
		);
		assert_eq!(on_identity.detailed_boot_type(), Some(MIONBootType::PCFS));
		assert_eq!(on_identity.detailed_is_cafe_on(), Some(true));
	}

	#[test]
	pub fn mion_announcement_ser_deser() {
		// Successes.
		{
			let announcement = MionIdentityAnnouncement { detailed: false };
			let serialized = Bytes::from(&announcement);
			let deser = MionIdentityAnnouncement::try_from(serialized);
			assert!(
				deser.is_ok(),
				"Failed to deserialize serialized MionIdentityAnnouncement!"
			);
			assert_eq!(
				announcement,
				deser.unwrap(),
				"MionIdentityAnnouncement was not the same after being serialized, and deserialized!",
			);
		}
		{
			let announcement = MionIdentityAnnouncement { detailed: true };
			let serialized = Bytes::from(&announcement);
			let deser = MionIdentityAnnouncement::try_from(serialized);
			assert!(
				deser.is_ok(),
				"Failed to deserialize serialized MionIdentityAnnouncement!"
			);
			assert_eq!(
				announcement,
				deser.unwrap(),
				"MionIdentityAnnouncement was not the same after being serialized, and deserialized!",
			);
		}

		// Packet not long enough.
		{
			let packet = Bytes::from(vec![
				// Command Byte
				u8::from(MionCommandByte::AnnounceYourselves),
				// Should be Message Buff.
				0xA,
				// NUL terminator.
				0x0,
			]);

			let result = MionIdentity::try_from((Ipv4Addr::LOCALHOST, packet.clone()));
			assert!(matches!(result, Err(NetworkError::ParseError(_))));
			// Guaranteed to be taken!
			if let Err(NetworkError::ParseError(value)) = result {
				assert_eq!(
					value,
					NetworkParseError::NotEnoughData("MionIdentity", 17, 3, packet)
				);
			}
		}

		// Packet too long.
		{
			let packet = Bytes::from(vec![
				// Command Byte
				u8::from(MionCommandByte::AnnounceYourselves),
				// Should be Message Buff.
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				// NUL terminator.
				0x0,
			]);

			let result = MionIdentityAnnouncement::try_from(packet.clone());
			assert!(matches!(result, Err(NetworkError::ParseError(_))));
			// Guaranteed to be taken!
			if let Err(NetworkError::ParseError(value)) = result {
				assert_eq!(
					value,
					NetworkParseError::UnexpectedTrailer(
						"MionIdentityAnnouncement",
						packet.slice(33..),
					)
				);
			}
		}

		// Command Byte Incorrect.
		{
			let mut buff = Vec::new();
			// Command byte incorrect
			buff.push(u8::from(MionCommandByte::Search));
			buff.extend_from_slice(ANNOUNCEMENT_MESSAGE.as_bytes());
			buff.push(0x0);
			let packet = Bytes::from(buff);

			let result = MionIdentityAnnouncement::try_from(packet);
			assert!(matches!(result, Err(NetworkError::ParseError(_))));
			// Guaranteed to be taken!
			if let Err(NetworkError::ParseError(value)) = result {
				assert_eq!(
					value,
					NetworkParseError::UnknownCommand(u8::from(MionCommandByte::Search))
				);
			}
		}

		// Packet Data incorrect data.
		{
			let packet = Bytes::from(vec![
				// The Command Byte.
				u8::from(MionCommandByte::AnnounceYourselves),
				// 23 bytes of bad data.
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				0xA,
				// NUL terminator.
				0x0,
			]);

			let result = MionIdentityAnnouncement::try_from(packet);
			assert!(matches!(result, Err(NetworkError::ParseError(_))));
			// Guaranteed to be taken!
			if let Err(NetworkError::ParseError(value)) = result {
				assert_eq!(value, NetworkParseError::FieldEncodedIncorrectly(
					"MionIdentityAnnouncement",
					"buff",
					"Must start with static message: `MULTI_I/O_NETWORK_BOARD` with a NUL Terminator",
				));
			}
		}

		// Not ending with a NUL terminator.
		{
			let mut buff = Vec::new();
			// Command byte incorrect
			buff.push(u8::from(MionCommandByte::AnnounceYourselves));
			buff.extend_from_slice(ANNOUNCEMENT_MESSAGE.as_bytes());
			// Not NUL byte.
			buff.push(0x1);
			let packet = Bytes::from(buff);

			let result = MionIdentityAnnouncement::try_from(packet);
			assert!(matches!(result, Err(NetworkError::ParseError(_))));
			// Guaranteed to be taken!
			if let Err(NetworkError::ParseError(value)) = result {
				assert_eq!(value, NetworkParseError::FieldEncodedIncorrectly(
					"MionIdentityAnnouncement",
					"buff",
					"Must start with static message: `MULTI_I/O_NETWORK_BOARD` with a NUL Terminator",
				));
			}
		}

		// `enumV1` tag is incorrect.
		{
			let mut buff = Vec::new();
			// Command byte incorrect
			buff.push(u8::from(MionCommandByte::AnnounceYourselves));
			buff.extend_from_slice(ANNOUNCEMENT_MESSAGE.as_bytes());
			buff.push(0x0);
			buff.extend_from_slice(DETAIL_FLAG_MESSAGE.as_bytes());
			// not null terminators.
			buff.push(0x1);
			buff.push(0x2);
			let packet = Bytes::from(buff);

			let result = MionIdentityAnnouncement::try_from(packet);
			assert!(matches!(result, Err(NetworkError::ParseError(_))));
			// Guaranteed to be taken!
			if let Err(NetworkError::ParseError(value)) = result {
				assert_eq!(value, NetworkParseError::FieldEncodedIncorrectly(
					"MionIdentityAnnouncement",
					"buff",
					"Only the static string `enumV1` followed by two NUL Terminators is allowed after `MULTI_I/O_NETWORK_BOARD`.",
				));
			}
		}
	}
}
