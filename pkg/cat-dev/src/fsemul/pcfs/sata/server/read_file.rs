//! Handle reading an already open file.

use crate::{
	errors::CatBridgeError,
	fsemul::{
		HostFilesystem,
		pcfs::sata::{
			proto::{SataReadFilePacketBody, SataRequest},
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
	let mut buffer_grew = flags.ffio_buffer_should_have_grown();

	if packet.should_move()
		&& let Err(cause) = packet
			.move_to_pointer()
			.do_move(&fs, handle, Some(stream.to_raw()))
			.await
	{
		debug!(
			?cause,
			packet.fd = handle,
			packet.typ = "PCFSSrvReadFile",
			"Failed to move file to a specific pointer!",
		);

		if ffio_enabled {
			return Ok(construct_ffio_error(FS_ERROR));
		}

		todo!("Implement non-FFIO support.");
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

	let first_read_size = usize::try_from(flags.first_read_size())
		.map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?;
	let total_read_amount = usize::try_from(packet.block_size())
		.map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?
		* usize::try_from(packet.block_count())
			.map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?;
	let Ok(Some(read_file)) = fs
		.read_file(handle, total_read_amount, Some(stream.to_raw()))
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
		let mut buff = BytesMut::with_capacity(0x24 + read_file.len());

		if !buffer_grew && read_file.len() > first_read_size {
			flags.set_ffio_buffer_should_have_grown(true);
			buffer_grew = true;
		}
		if (read_file.len() < total_read_amount && read_file.len() > first_read_size) || buffer_grew
		{
			buff.extend_from_slice(&[0; 0x20]);
			buff.put_u32(u32::try_from(read_file.len()).unwrap_or(u32::MAX));
		} else {
			buff.extend_from_slice(&[0xC4, 0x00, 0x24, 0x02, 0xE8, 0xEF, 0x24, 0x02]);
			buff.extend_from_slice(&[0; 0x18]);
			buff.put_u32(u32::try_from(file_size).unwrap_or(u32::MAX));
		}

		let rf_len = read_file.len();
		buff.extend(read_file);

		if rf_len < total_read_amount && rf_len < first_read_size {
			let pad_amount = std::cmp::min(total_read_amount, first_read_size) - rf_len;
			buff.extend(vec![0xCD; pad_amount]);
		}

		Ok(buff.freeze())
	} else {
		todo!("Implement non-FFIO support.")
	}
}

fn construct_ffio_error(error_code: u32) -> Bytes {
	let mut buff = BytesMut::with_capacity(36);
	buff.extend(&[0xC4, 0x00, 0xFE, 0x00, 0x20, 0xEF, 0xFE, 0x00]);
	buff.extend([0; 0x18]);
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
		let conn_flags = SataConnectionFlags::new_with_flags(true, true);
		conn_flags.set_first_read_size(1000);
		conn_flags.set_first_write_size(1000);

		let response = handle_read_file(
			StreamID::from_existing(1),
			conn_flags,
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
		expected_response.extend_from_slice(&[0xC4, 0x00, 0x24, 0x02, 0xE8, 0xEF, 0x24, 0x02]);
		expected_response.extend_from_slice(&[0; 0x18]);
		// File length.
		expected_response.extend_from_slice(&2_u32.to_be_bytes());
		// File data, and padding.
		expected_response.extend_from_slice(&[0x00, 0x00, 0xCD, 0xCD]);
		assert_eq!(response, expected_response.freeze());
	}
}
