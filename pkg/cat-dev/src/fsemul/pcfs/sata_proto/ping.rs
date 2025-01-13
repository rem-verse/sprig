//! Definitions, and handlers for the 'PING' packet type.
//!
//! Ping packets are much like ping packet types in any sort of scenario, they
//! are built to check availability. Not to mention ping packets confer what
//! features are enabled.

use crate::{
	errors::NetworkParseError,
	fsemul::pcfs::{
		errors::PCFSApiError,
		sata_proto::{construct_sata_response, SataCommandInfo, SataPacketHeader},
	},
};
use bytes::{BufMut, Bytes, BytesMut};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// A ZST that represents a ping packet coming in.
#[derive(Clone, Debug, PartialEq, Eq, Valuable)]
pub struct SataPingPacketBody;

impl SataPingPacketBody {
	/// Handle a ping packet.
	///
	/// ## Errors
	///
	/// Should never error, but could error if we fail to construct the response
	/// for some reason.
	pub fn handle(
		&self,
		request_header: &SataPacketHeader,
		command_info: &SataCommandInfo,
		server_and_client_supports_ffio: bool,
		server_and_client_supports_csr: bool,
	) -> Result<Bytes, PCFSApiError> {
		let supports_ffio = command_info.capabilities.0 != 0 && server_and_client_supports_ffio;
		let supports_csr = command_info.capabilities.0 != 0 && server_and_client_supports_csr;

		construct_sata_response(
			request_header,
			0,
			SataPongBody::new(supports_ffio, supports_csr),
		)
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

impl From<SataPongBody> for Bytes {
	fn from(value: SataPongBody) -> Self {
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

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[test]
	pub fn can_respond_to_ping() {
		let ping = SataPingPacketBody;

		let example_ping_packet_header = SataPacketHeader {
			packet_data_len: 0,
			packet_id: 0,
			flags: 0,
			version: 0,
			timestamp_on_host: 0,
			pid_on_host: 0,
		};
		let example_command_info = SataCommandInfo {
			user: (0, 0),
			capabilities: (0, 1),
			command: 0x14,
		};

		let all_supported = ping
			.handle(
				&example_ping_packet_header,
				&example_command_info,
				true,
				true,
			)
			.expect("Failed to handle ping with all features enabled!");
		let only_ffio_supported = ping
			.handle(
				&example_ping_packet_header,
				&example_command_info,
				true,
				false,
			)
			.expect("Failed to handle ping with only ffio features enabled!");
		let only_csr_supported = ping
			.handle(
				&example_ping_packet_header,
				&example_command_info,
				false,
				true,
			)
			.expect("Failed to handle ping with only csr enabled!");
		let none_supported = ping
			.handle(
				&example_ping_packet_header,
				&example_command_info,
				false,
				false,
			)
			.expect("Failed to handle ping with only csr enabled!");

		assert!(all_supported.ends_with(&[0xCA, 0xFE, 0x00, 0x03]));
		assert!(only_ffio_supported.ends_with(&[0xCA, 0xFE, 0x00, 0x01]));
		assert!(only_csr_supported.ends_with(&[0xCA, 0xFE, 0x00, 0x02]));
		assert!(none_supported.ends_with(&[0x00, 0x00, 0x00, 0x00]));
	}
}
