//! Handlers for ping packets.
//!
//! Ping packets are interesting because they also can affect connection level
//! state such as FFIO Enablement, CSR Enablement, etc.

use crate::{
	fsemul::pcfs::sata::{
		proto::{
			SataCommandInfo, SataPacketHeader, SataPingPacketBody, SataPongBody, SataResponse,
		},
		server::connection_flags::SataConnectionFlags,
	},
	net::server::requestable::{Body, State},
};
use tracing::{debug, field::valuable};

/// Handle a ping request coming in.
pub async fn handle_ping(
	flags: SataConnectionFlags,
	header: SataPacketHeader,
	command_info: SataCommandInfo,
	State(pid): State<u32>,
	// Deserialize to validate the body is empty, but don't use it.
	Body(ping): Body<SataPingPacketBody>,
) -> SataResponse<SataPongBody> {
	debug!(
		client.packet.header = valuable(&header),
		client.packet.command_info = valuable(&command_info),
		client.packet.body = valuable(&ping),
		"received ping packet from client",
	);

	if command_info.capabilities().0 == 0 {
		flags.set_csr_enabled(false);
		flags.set_ffio_enabled(false);
	}

	SataResponse::new(
		pid,
		header,
		SataPongBody::new(flags.ffio_enabled(), flags.csr_enabled()),
	)
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use bytes::Bytes;

	#[tokio::test]
	pub async fn can_respond_to_ping() {
		let ping = SataPingPacketBody;
		let example_header = SataPacketHeader::new(0);
		let example_command_info = SataCommandInfo::new((0, 0), (1, 0), 0x14);

		let all_supported: Bytes = handle_ping(
			SataConnectionFlags::new_with_flags(true, true),
			example_header.clone(),
			example_command_info.clone(),
			State(1_u32),
			Body(ping),
		)
		.await
		.try_into()
		.expect("Failed to serialize pong with all features enabled!");
		let only_ffio_supported: Bytes = handle_ping(
			SataConnectionFlags::new_with_flags(true, false),
			example_header.clone(),
			example_command_info.clone(),
			State(1_u32),
			Body(ping),
		)
		.await
		.try_into()
		.expect("Failed to serialize pong with only ffio enabled!");
		let only_csr_supported: Bytes = handle_ping(
			SataConnectionFlags::new_with_flags(false, true),
			example_header.clone(),
			example_command_info.clone(),
			State(1_u32),
			Body(ping),
		)
		.await
		.try_into()
		.expect("Failed to serialize pong with only csr enabled!");
		let none_supported: Bytes = handle_ping(
			SataConnectionFlags::new_with_flags(false, false),
			example_header.clone(),
			example_command_info.clone(),
			State(1_u32),
			Body(ping),
		)
		.await
		.try_into()
		.expect("Failed to serialize pong with no features enabled!");

		assert!(all_supported.ends_with(&[0xCA, 0xFE, 0x00, 0x03]));
		assert!(only_ffio_supported.ends_with(&[0xCA, 0xFE, 0x00, 0x01]));
		assert!(only_csr_supported.ends_with(&[0xCA, 0xFE, 0x00, 0x02]));
		assert!(none_supported.ends_with(&[0x00, 0x00, 0x00, 0x00]));
	}
}
