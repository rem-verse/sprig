//! Handle changing the ownership of a file.
//!
//! For actual PCFS implemented on a real hardware, change owner is not
//! implemented. We may one day actually implement an extension to properly
//! set the owner/group of a file.

use crate::{
	fsemul::pcfs::sata::proto::{
		SataChangeOwnerPacketBody, SataRequest, SataResponse, SataResultCode,
	},
	net::server::requestable::{Body, State},
};

/// Handle a change owner request.
///
/// This will always fail due to the original implementation also always
/// failing.
pub async fn handle_change_owner(
	State(pid): State<u32>,
	// Validate that the body is actually a change owner packet.
	Body(request): Body<SataRequest<SataChangeOwnerPacketBody>>,
) -> SataResponse<SataResultCode> {
	SataResponse::new(
		pid,
		request.header().clone(),
		SataResultCode::error(0xFFF0_FFE0),
	)
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use crate::fsemul::pcfs::sata::proto::{SataCommandInfo, SataPacketHeader, SataRequest};
	use bytes::Bytes;

	#[tokio::test]
	pub async fn change_owner_request() {
		let request =
			SataChangeOwnerPacketBody::new("/%SLC_EMU_DIR/to-query/file.txt".to_owned(), 0, 0)
				.expect("Failed to construct sata change owner packet body!");
		let mocked_header = SataPacketHeader::new(0);
		let mocked_ci = SataCommandInfo::new((0, 0), (0, 0), 0);

		let mut response: Bytes = handle_change_owner(
			State(0),
			Body(SataRequest::new(mocked_header, mocked_ci, request)),
		)
		.await
		.try_into()
		.expect("Failed to serialize change owner response!");
		assert_eq!(response.len(), 4 + 0x20, "Packet is not correct size!");
		// Okay first chop off the header, we don't care.
		_ = response.split_to(0x20);
		assert_eq!(
			response,
			Bytes::from(vec![
				0xFF, 0xF0, 0xFF, 0xE0, // RC
			]),
		);
	}
}
