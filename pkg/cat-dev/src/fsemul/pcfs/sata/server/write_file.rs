//! Handle writing a file to the host PC disk.

use crate::{
	errors::CatBridgeError,
	fsemul::pcfs::{
		errors::PCFSApiError,
		sata::{
			proto::{
				MoveToFileLocation, SataRequest, SataResponse, SataResultCode,
				SataWriteFilePacketBody,
			},
			server::{PCFSServerState, SataConnectionFlags},
		},
	},
	net::models::Request,
};
use tracing::debug;

/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;

/// Handle writing to a file that is already open.
///
/// ## Errors
///
/// If we are not running on at least a 32bit based OS, missing a critical
/// extension, cannot parse the body as a [`SataWriteFilePacketBody`], or
/// if the stream is no longer around to be read around.
pub async fn handle_write_file(
	req: Request<PCFSServerState>,
) -> Result<SataResponse<SataResultCode>, CatBridgeError> {
	let flags = req
		.extensions()
		.get::<SataConnectionFlags>()
		.cloned()
		.ok_or_else(|| PCFSApiError::MissingCriticalExtension("SataConnectionFlags".to_owned()))?;
	let state = req.state();
	let request = SataRequest::<SataWriteFilePacketBody>::try_from(req.body().clone())?;
	let request_header = request.header().clone();
	let packet = request.body();

	if packet.should_move() {
		match packet.move_to_pointer() {
			MoveToFileLocation::Begin => {
				if state
					.host_filesystem()
					.seek_file(packet.file_descriptor(), true, Some(req.stream_id()))
					.await
					.is_err()
				{
					debug!(
						packet.fd = packet.file_descriptor(),
						packet.typ = "PCFSSrvWriteFile",
						"Failed to seek to beginning of file!",
					);

					return Ok(SataResponse::new(
						state.pid(),
						request_header,
						SataResultCode::error(FS_ERROR),
					));
				}
			}
			MoveToFileLocation::Current => {
				// Luckily to move to current, we don't need to move at all.
			}
			MoveToFileLocation::End => {
				if state
					.host_filesystem()
					.seek_file(packet.file_descriptor(), false, Some(req.stream_id()))
					.await
					.is_err()
				{
					debug!(
						packet.fd = packet.file_descriptor(),
						packet.typ = "PCFSSrvWriteFile",
						"Failed to seek to end of file!",
					);

					return Ok(SataResponse::new(
						state.pid(),
						request_header,
						SataResultCode::error(FS_ERROR),
					));
				}
			}
		}
	}

	if flags.ffio_enabled() {
		let len_needed = usize::try_from(packet.block_count() * packet.block_size())
			.map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?;
		// Bypass header and such checks...
		let buff = req.unsafe_read_more_bytes_from_stream(len_needed).await?;
		state
			.host_filesystem
			.write_file(packet.file_descriptor(), buff, Some(req.stream_id()))
			.await?;

		Ok(SataResponse::new(
			state.pid(),
			request_header,
			SataResultCode::success(),
		))
	} else {
		todo!("Implement non-FFIO support.")
	}
}
