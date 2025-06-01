//! Handlers for ping packets.
//!
//! Ping packets are interesting because they also can affect connection level
//! state such as FFIO Enablement, CSR Enablement, etc.

use crate::{
	fsemul::pcfs::sata::{
		proto::{
			SataCapabilitiesFlags, SataPingPacketBody, SataPongBody, SataRequest, SataResponse,
		},
		server::connection_flags::SataConnectionFlags,
	},
	net::server::requestable::{Body, State},
};
use tracing::{debug, field::valuable};

/// Handle a ping request coming in.
pub async fn handle_ping(
	flags: SataConnectionFlags,
	State(pid): State<u32>,
	// Deserialize to validate the body is empty, but don't use it.
	Body(request): Body<SataRequest<SataPingPacketBody>>,
) -> SataResponse<SataPongBody> {
	let header = request.header();
	let command_info = request.command_info();
	let ping = request.body();

	debug!(
		client.packet.header = valuable(header),
		client.packet.command_info = valuable(command_info),
		client.packet.body = valuable(ping),
		"received ping packet from client",
	);

	let capabilities = SataCapabilitiesFlags(header.flags());
	if !capabilities.intersects(SataCapabilitiesFlags::FAST_FILE_IO_SUPPORTED) {
		flags.set_ffio_enabled(false);
	}
	if !capabilities.intersects(SataCapabilitiesFlags::COMBINED_SEND_RECV_SUPPORTED) {
		flags.set_csr_enabled(false);
	}
	if command_info.capabilities().0 == 0 {
		flags.set_csr_enabled(false);
		flags.set_ffio_enabled(false);
	}

	SataResponse::new(
		pid,
		header.clone(),
		SataPongBody::new(flags.ffio_enabled(), flags.csr_enabled()),
	)
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use crate::fsemul::pcfs::sata::proto::{SataCommandInfo, SataPacketHeader};
	use bytes::Bytes;

	#[tokio::test]
	pub async fn can_respond_to_ping() {
		let ping = SataPingPacketBody;
		let mut example_header = SataPacketHeader::new(0);
		example_header.set_flags(SataCapabilitiesFlags::all().0);
		let example_command_info = SataCommandInfo::new((0, 0), (1, 0), 0x14);

		let all_supported: Bytes = handle_ping(
			SataConnectionFlags::new_with_flags(true, true),
			State(1_u32),
			Body(SataRequest::new(
				example_header.clone(),
				example_command_info.clone(),
				ping,
			)),
		)
		.await
		.try_into()
		.expect("Failed to serialize pong with all features enabled!");
		let only_ffio_supported: Bytes = handle_ping(
			SataConnectionFlags::new_with_flags(true, false),
			State(1_u32),
			Body(SataRequest::new(
				example_header.clone(),
				example_command_info.clone(),
				ping,
			)),
		)
		.await
		.try_into()
		.expect("Failed to serialize pong with only ffio enabled!");
		let only_csr_supported: Bytes = handle_ping(
			SataConnectionFlags::new_with_flags(false, true),
			State(1_u32),
			Body(SataRequest::new(
				example_header.clone(),
				example_command_info.clone(),
				ping,
			)),
		)
		.await
		.try_into()
		.expect("Failed to serialize pong with only csr enabled!");
		let none_supported: Bytes = handle_ping(
			SataConnectionFlags::new_with_flags(false, false),
			State(1_u32),
			Body(SataRequest::new(
				example_header.clone(),
				example_command_info.clone(),
				ping,
			)),
		)
		.await
		.try_into()
		.expect("Failed to serialize pong with no features enabled!");

		assert!(
			all_supported.ends_with(&[0xCA, 0xFE, 0x00, 0x03]),
			"Expected all supported to end with 0xCAFE_0003, but was:\n\n  {:02x?}\n\n",
			all_supported,
		);
		assert!(only_ffio_supported.ends_with(&[0xCA, 0xFE, 0x00, 0x01]));
		assert!(only_csr_supported.ends_with(&[0xCA, 0xFE, 0x00, 0x02]));
		assert!(none_supported.ends_with(&[0x00, 0x00, 0x00, 0x00]));
	}
}
