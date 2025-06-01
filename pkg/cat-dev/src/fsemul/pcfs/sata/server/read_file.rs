//! Handle reading an already open file.

use crate::{
	errors::CatBridgeError,
	fsemul::{
		HostFilesystem,
		pcfs::sata::{
			proto::{MoveToFileLocation, SataReadFilePacketBody, SataRequest},
			server::SataConnectionFlags,
		},
	},
	net::{
		additions::StreamID,
		server::requestable::{Body, State},
	},
};
use bytes::{BufMut, Bytes, BytesMut};
use tracing::debug;

/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;

/// Handle reading from a file that is already open.
///
/// ## Errors
///
/// If we cannot construct a sata response packet because our data to send
/// was somehow too large (this should ideally never happen), or if we're
/// running on a 16 bit system.
pub async fn handle_read_file(
	stream: StreamID,
	flags: SataConnectionFlags,
	State(fs): State<HostFilesystem>,
	Body(request): Body<SataRequest<SataReadFilePacketBody>>,
) -> Result<Bytes, CatBridgeError> {
	let packet = request.body();
	let handle = packet.file_descriptor();
	let ffio_enabled = flags.ffio_enabled();

	if packet.should_move() {
		match packet.move_to_pointer() {
			MoveToFileLocation::Begin => {
				if fs
					.seek_file(handle, true, Some(stream.to_raw()))
					.await
					.is_err()
				{
					debug!(
						packet.fd = handle,
						packet.typ = "PCFSSrvReadFile",
						"Failed to seek to beginning of file!",
					);

					if ffio_enabled {
						return Ok(construct_ffio_error(FS_ERROR));
					}

					todo!("Implement non-FFIO support.");
				}
			}
			MoveToFileLocation::Current => {
				// Luckily to move to current, we don't need to move at all.
			}
			MoveToFileLocation::End => {
				if fs
					.seek_file(handle, false, Some(stream.to_raw()))
					.await
					.is_err()
				{
					debug!(
						packet.fd = handle,
						packet.typ = "PCFSSrvReadFile",
						"Failed to seek to end of file of file!",
					);

					if ffio_enabled {
						return Ok(construct_ffio_error(FS_ERROR));
					}
					todo!("Implement non-FFIO support.");
				}
			}
		}
	}

	let Some(file_size) = fs.file_length(handle, Some(stream.to_raw())).await else {
		debug!(
			packet.fd = handle,
			packet.typ = "PCFSSrvReadFile",
			"Failed to query length of file!",
		);

		if ffio_enabled {
			return Ok(construct_ffio_error(FS_ERROR));
		}
		todo!("Implement non-ffio support.");
	};
	let Ok(Some(read_file)) = fs
		.read_file(
			handle,
			usize::try_from(packet.block_size())
				.map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?
				* usize::try_from(packet.block_count())
					.map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?,
			Some(stream.to_raw()),
		)
		.await
	else {
		debug!(
			packet.fd = handle,
			packet.typ = "PCFSSrvReadFile",
			"Failed to read bytes of from file!",
		);

		if ffio_enabled {
			return Ok(construct_ffio_error(FS_ERROR));
		}
		todo!("Implement non-ffio support.");
	};

	if ffio_enabled {
		let mut buff = BytesMut::with_capacity(read_file.len() + 0x24);
		// The header is normally just 'malloc'd and not cleared between
		// buffers. Luckily for us we can just zero it out, and it's easier than
		// actually dealing with whatever random bytes PCFSServer would normally
		// send.
		buff.extend_from_slice(&[0; 0x20]);
		buff.put_u32(u32::try_from(file_size).unwrap_or(u32::MAX));
		buff.extend(read_file);

		Ok(buff.freeze())
	} else {
		todo!("Implement non-FFIO support.")
	}
}

fn construct_ffio_error(error_code: u32) -> Bytes {
	let mut buff = BytesMut::with_capacity(36);
	buff.extend(&[0xC4, 0x00, 0xFE, 0x00, 0x20, 0xEF, 0xFE, 0x00]);
	buff.extend([0; 24]);
	buff.put_u32(error_code);
	buff.freeze()
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use crate::fsemul::{
		host_filesystem::test_helpers::{create_temporary_host_filesystem, join_many},
		pcfs::sata::proto::{SataCommandInfo, SataPacketHeader},
	};
	use tokio::fs::OpenOptions;

	#[tokio::test]
	pub async fn simple_ffio_read_file_request() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;

		let base_dir = join_many(tempdir.path(), ["data", "slc", "to-query"]);
		tokio::fs::create_dir(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		tokio::fs::write(join_many(&base_dir, ["file.txt"]), vec![0; 2])
			.await
			.expect("Failed to write test file!");

		let mut open_options = OpenOptions::new();
		open_options.read(true).create(false).write(false);
		let fd = fs
			.open_file(open_options, &join_many(&base_dir, ["file.txt"]), Some(1))
			.await
			.expect("Failed to open file!");

		let read_request = SataReadFilePacketBody::new(4, 1, fd, None);

		let response = handle_read_file(
			StreamID::from_existing(1),
			SataConnectionFlags::new_with_flags(true, true),
			State(fs),
			Body(SataRequest::new(
				SataPacketHeader::new(0),
				SataCommandInfo::new((0, 0), (0, 0), 0),
				read_request,
			)),
		)
		.await
		.expect("Failed to handle read file!");

		let mut expected_response = BytesMut::new();
		// Header
		expected_response.extend_from_slice(&[0; 0x20]);
		// File length.
		expected_response.extend_from_slice(&2_u32.to_be_bytes());
		// File data, and padding.
		expected_response.extend_from_slice(&[0x00, 0x00, 0xCD, 0xCD]);
		assert_eq!(response, expected_response.freeze());
	}
}
