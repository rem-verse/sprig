//! Handle writing a file to the host PC disk.

use crate::{
	errors::CatBridgeError,
	fsemul::pcfs::{
		errors::PCFSApiError,
		sata::{
			proto::{SataRequest, SataResponse, SataResultCode, SataWriteFilePacketBody},
			server::{PCFSServerState, SataConnectionFlags, wal::WriteAheadLog},
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
	let opt_wal = req.extensions().get::<WriteAheadLog>();
	let state = req.state();
	let request = SataRequest::<SataWriteFilePacketBody>::try_from(req.body().clone())?;
	let request_header = request.header().clone();
	let packet = request.body();

	if packet.should_move()
		&& let Err(cause) = packet
			.move_to_pointer()
			.do_move(
				state.host_filesystem(),
				packet.file_descriptor(),
				Some(req.stream_id()),
			)
			.await
	{
		debug!(
			?cause,
			packet.fd = packet.file_descriptor(),
			packet.typ = "PCFSSrvWriteFile",
			"Failed to seek file!",
		);

		return Ok(SataResponse::new(
			state.pid(),
			request_header,
			SataResultCode::error(FS_ERROR),
		));
	}

	if flags.ffio_enabled() {
		let len_needed = usize::try_from(packet.block_count() * packet.block_size())
			.map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?;
		// Bypass header and such checks...
		let buff = req.unsafe_read_more_bytes_from_stream(len_needed).await?;

		if let Some(wal) = opt_wal {
			wal.record_oob_file_write_read(req.stream_id(), packet.file_descriptor(), len_needed)
				.await;
		}

		if let Err(cause) = state
			.host_filesystem
			.write_file(packet.file_descriptor(), buff, Some(req.stream_id()))
			.await
		{
			debug!(
				?cause,
				packet.fd = packet.file_descriptor(),
				packet.typ = "PCFSSrvWriteFile",
				"Failed to write file!",
			);

			return Ok(SataResponse::new(
				state.pid(),
				request_header,
				SataResultCode::error(FS_ERROR),
			));
		}

		Ok(SataResponse::new_force_zero_version(
			state.pid(),
			request_header,
			// Abuse result code, we actually write the total amount of bytes written.
			SataResultCode::error(packet.block_size() * packet.block_count()),
		))
	} else {
		todo!("Implement non-FFIO support.")
	}
}
