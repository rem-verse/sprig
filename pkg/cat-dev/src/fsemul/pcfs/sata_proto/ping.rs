//! Definitions, and handlers for the 'PING' packet type.
//!
//! Ping packets are much like ping packet types in any sort of scenario, they
//! are built to check availability. Not to mention ping packets confer what
//! features are enabled.

use crate::errors::NetworkParseError;
use bytes::Bytes;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PCFSPingPacketBody {
	/// If this is set the client MUST not use either FFIO, or combined
	/// send/recv.
	force_no_ffio_or_combined_send_recv: bool,
	/// Logged as first read size, but unsure exactly what this means.
	first_read_size: u32,
	/// Logged as first write size, but unsure exactly what this means.
	first_write_size: u32,
	/// Logged as FFIO Version, unsure exactly what the versions are.
	ffio_version: u32,
}

impl TryFrom<Bytes> for PCFSPingPacketBody {
	type Error = NetworkParseError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 16 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"PCFSSata",
				"PingBody",
				16,
				value.len(),
				value,
			));
		}
		if value.len() > 16 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"PCFSSataPing",
				value.slice(16..),
			));
		}

		let first_read_size = u32::from_be_bytes([value[0], value[1], value[2], value[3]]);
		let first_write_size = u32::from_be_bytes([value[4], value[5], value[6], value[7]]);
		let ffio_version = u32::from_be_bytes([value[8], value[9], value[10], value[11]]);
		let force_no_ffio_or_csr =
			u32::from_be_bytes([value[12], value[13], value[14], value[15]]) != 0;

		Ok(Self {
			force_no_ffio_or_combined_send_recv: force_no_ffio_or_csr,
			first_read_size,
			first_write_size,
			ffio_version,
		})
	}
}

const PCFS_PING_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("force_no_ffio_or_combined_send_recv"),
	NamedField::new("first_read_size"),
	NamedField::new("first_write_size"),
	NamedField::new("ffio_version"),
];

impl Structable for PCFSPingPacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"PCFSPingPacketBody",
			Fields::Named(PCFS_PING_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for PCFSPingPacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			PCFS_PING_PACKET_BODY_FIELDS,
			&[
				Valuable::as_value(&self.force_no_ffio_or_combined_send_recv),
				Valuable::as_value(&self.first_read_size),
				Valuable::as_value(&self.first_write_size),
				Valuable::as_value(&self.ffio_version),
			],
		));
	}
}
