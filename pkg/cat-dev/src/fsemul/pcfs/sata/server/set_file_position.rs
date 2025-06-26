//! Handle a client requesting us to move the location of an open file.

use crate::{
	fsemul::pcfs::sata::{
		proto::{SataRequest, SataResponse, SataResultCode, SataSetFilePositionPacketBody},
		server::PCFSServerState,
	},
	net::{
		additions::StreamID,
		server::requestable::{Body, State},
	},
};
use tracing::debug;

/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;

/// Handle setting the position within an already open file.
pub async fn handle_set_file_position(
	stream: StreamID,
	State(state): State<PCFSServerState>,
	// Validate that the body is actually a change owner packet.
	Body(request): Body<SataRequest<SataSetFilePositionPacketBody>>,
) -> SataResponse<SataResultCode> {
	let packet = request.body();

	if let Err(cause) = packet
		.move_to_pointer()
		.do_move(
			state.host_filesystem(),
			packet.file_descriptor(),
			Some(stream.to_raw()),
		)
		.await
	{
		debug!(
			?cause,
			packet.fd = packet.file_descriptor(),
			packet.typ = "PCFSSrvSetFilePosition",
			"Failed to move file to a specific pointer!",
		);

		SataResponse::new(
			state.pid(),
			request.header().clone(),
			SataResultCode::error(FS_ERROR),
		)
	} else {
		SataResponse::new(
			state.pid(),
			request.header().clone(),
			SataResultCode::success(),
		)
	}
}
