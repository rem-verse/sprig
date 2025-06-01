//! Definitions for the `PING` packet type, and it's response types.
//!
//! Ping packets are much like ping packet types in any sort of scenario, they
//! are built to check availability. Not to mention ping packets confer what
//! features are enabled.

use crate::errors::NetworkParseError;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// A ZST that represents a ping packet coming in.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Valuable)]
pub struct SataPingPacketBody;

impl SataPingPacketBody {
	#[must_use]
	pub const fn new() -> Self {
		Self
	}
}

impl Default for SataPingPacketBody {
	fn default() -> Self {
		Self
	}
}

impl TryFrom<Bytes> for SataPingPacketBody {
	type Error = NetworkParseError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if !value.is_empty() {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataPingPacketBody",
				value,
			));
		}

		Ok(Self)
	}
}

impl From<&SataPingPacketBody> for Bytes {
	fn from(_: &SataPingPacketBody) -> Self {
		Bytes::new()
	}
}

impl From<SataPingPacketBody> for Bytes {
	fn from(_: SataPingPacketBody) -> Self {
		Bytes::new()
	}
}

/// A response to a `PING` over the Sata protocol of `PCFS`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataPongBody {
	/// If Fast File I/O is enabled and supported by both sides.
	fast_file_io_enabled: bool,
	/// If Combined Send/Recv is enabled and supported by both sides.
	combined_send_recv_enabled: bool,
}

impl SataPongBody {
	#[must_use]
	pub const fn new(ffio_enabled: bool, csr_enabled: bool) -> Self {
		Self {
			fast_file_io_enabled: ffio_enabled,
			combined_send_recv_enabled: csr_enabled,
		}
	}

	#[must_use]
	pub const fn ffio_enabled(&self) -> bool {
		self.fast_file_io_enabled
	}

	pub const fn set_ffio_enabled(&mut self, enabled: bool) {
		self.fast_file_io_enabled = enabled;
	}

	#[must_use]
	pub const fn combined_send_recv_enabled(&self) -> bool {
		self.combined_send_recv_enabled
	}

	pub const fn set_combined_send_recv_enabled(&mut self, enabled: bool) {
		self.combined_send_recv_enabled = enabled;
	}
}

impl From<&SataPongBody> for Bytes {
	fn from(value: &SataPongBody) -> Self {
		let mut buff = BytesMut::with_capacity(8);

		buff.put_u32(0x0); // Success! - This is a return code.
		buff.put_u32(
			match (value.fast_file_io_enabled, value.combined_send_recv_enabled) {
				(true, true) => 0xCAFE_0003,
				(true, false) => 0xCAFE_0001,
				(false, true) => 0xCAFE_0002,
				(false, false) => 0x0000_0000,
			},
		);

		buff.freeze()
	}
}

impl From<SataPongBody> for Bytes {
	fn from(value: SataPongBody) -> Self {
		Self::from(&value)
	}
}

impl TryFrom<Bytes> for SataPongBody {
	type Error = NetworkParseError;

	fn try_from(mut value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x8 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataPongBody",
				"Body",
				0x8,
				value.len(),
				value,
			));
		}
		if value.len() > 0x8 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataPongBody",
				value.slice(0x8..),
			));
		}

		// Get the return code.
		let rc = value.get_u32();
		if rc != 0 {
			return Err(NetworkParseError::ErrorCode(rc));
		}
		let flags = value.get_u32();

		Ok(Self {
			fast_file_io_enabled: flags == 0xCAFE_0003 || flags == 0xCAFE_0001,
			combined_send_recv_enabled: flags == 0xCAFE_0003 || flags == 0xCAFE_0002,
		})
	}
}

const SATA_PONG_BODY_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("fast_file_io_enabled"),
	NamedField::new("combined_send_recv_enabled"),
];

impl Structable for SataPongBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static("SataPongBody", Fields::Named(SATA_PONG_BODY_FIELDS))
	}
}

impl Valuable for SataPongBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_PONG_BODY_FIELDS,
			&[
				Valuable::as_value(&self.fast_file_io_enabled),
				Valuable::as_value(&self.combined_send_recv_enabled),
			],
		));
	}
}
