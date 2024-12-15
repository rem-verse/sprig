//! Functions related to handling reads from the SDIO interface.
//!
//! These are the bits that handle all of the translation from an SDIO packet
//! to a real file being sent back. They mostly are just wrappers around
//! [`HostFilesystem`] calls, being wrapped in SDIO protocols.

use crate::{
	errors::{CatBridgeError, FSError, NetworkError},
	fsemul::{
		dlf::DiskLayoutFile,
		sdio::{
			errors::SDIOProtocolError,
			proto::{SdioControlReadRequest, SDIO_BLOCKS_PER_PACKET, SDIO_BLOCK_SIZE},
		},
		HostFilesystem,
	},
};
use bytes::{Bytes, BytesMut};
use std::path::PathBuf;
use tokio::{
	fs::{read as fs_read, File},
	io::{AsyncReadExt, BufReader},
	sync::mpsc::Sender,
};
use tracing::{info, warn};

/// Actually do all the bits to serve a read request, and respond over the
/// passed in channel.
///
/// ## Errors
///
/// - If the device requests an unknown address to read files from.
/// - If we cannot read the file from the disk.
/// - If we cannot serve the file to a client.
pub async fn serve_read_request(
	file_system: &HostFilesystem,
	request: &SdioControlReadRequest,
	response_channel: &Sender<Bytes>,
) -> Result<(), CatBridgeError> {
	let address_to_read = request.lba();
	if address_to_read == 0xFFFF_0000 {
		info!("Requested special ppc_boot.bsf file address");
		let ppc_boot = file_system.boot1_sytstem_path().await?;
		return serve_padded_file_sdio(&ppc_boot, request.blocks(), response_channel).await;
	// PRobably another special address like diskid or something.
	} else if address_to_read == 0x00F9_0000 {
		info!("Unknown special large address... serving 0 blocks");
		return serve_zeroed_blocks(request.blocks(), response_channel).await;
	}

	let dlf = DiskLayoutFile::try_from(Bytes::from(
		fs_read(file_system.ppc_boot_dlf_path().await?)
			.await
			.map_err(FSError::from)?,
	))?;
	if u128::from(address_to_read) > dlf.max_address() {
		return Err(SDIOProtocolError::AddressOutOfRange(
			u128::from(address_to_read),
			dlf.max_address(),
		)
		.into());
	}

	if let Some(path) = dlf.get_path_for_address(u128::from(address_to_read)) {
		info!(
			sdio.blocks = request.blocks(),
			sdio.path = %path.display(),
			"Serving known file over SDIO",
		);
		serve_padded_file_sdio(path, request.blocks(), response_channel).await
	} else {
		warn!(
			sdio.address = address_to_read,
			sdio.blocks = request.blocks(),
			"Serving unknown address over SDIO",
		);
		serve_zeroed_blocks(request.blocks(), response_channel).await
	}
}

/// Serve a file over a tcp stream to SDIO.
///
/// Take in a path to a file, the blocks a user is requesting, and serve that
/// many blocks. If the `blocks_requested` is greater than the contents of
/// `path`, then we will just serve zero's past the point to fulfill the amount
/// of `blocks_requested`.
///
/// ## Errors
///
/// - If we cannot open a file at `path`.
/// - If you are not running on at least a 32 bit machine, and we cannot turn
///   `blocks_requested` into a `usize`.
/// - If we cannot send content over the `response_channel`.
async fn serve_padded_file_sdio(
	path: &PathBuf,
	blocks_requested: u32,
	response_channel: &Sender<Bytes>,
) -> Result<(), CatBridgeError> {
	let mut fd = File::open(path).await.map_err(FSError::IO)?;

	let mut blocks_as_size =
		usize::try_from(blocks_requested).map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?;
	// Small enough, ready to just be read one-shot.
	if blocks_as_size <= SDIO_BLOCKS_PER_PACKET {
		let mut file_buff = BytesMut::with_capacity(blocks_as_size * SDIO_BLOCK_SIZE);
		let read_bytes = fd.read_buf(&mut file_buff).await.map_err(FSError::IO)?;
		if read_bytes < blocks_as_size * SDIO_BLOCK_SIZE {
			let padding = BytesMut::zeroed((blocks_as_size * SDIO_BLOCK_SIZE) - read_bytes);
			file_buff.extend(padding);
		}
		response_channel
			.send(file_buff.freeze())
			.await
			.map_err(NetworkError::SendQueueFailure)?;
	} else {
		let mut exhausted_file = false;
		let mut reader = BufReader::new(fd);

		while blocks_as_size > 0 {
			let blocks_to_read = std::cmp::min(blocks_as_size, SDIO_BLOCKS_PER_PACKET);
			let bytes_to_read = blocks_to_read * SDIO_BLOCK_SIZE;
			let mut file_buff = BytesMut::with_capacity(bytes_to_read);

			let read_bytes = if exhausted_file {
				0
			} else {
				let read_bytes = reader.read_buf(&mut file_buff).await.map_err(FSError::IO)?;
				if read_bytes == 0 {
					exhausted_file = true;
				}
				read_bytes
			};

			if read_bytes < bytes_to_read {
				let padding = BytesMut::zeroed(bytes_to_read - read_bytes);
				file_buff.extend(padding);
			}

			response_channel
				.send(file_buff.freeze())
				.await
				.map_err(NetworkError::SendQueueFailure)?;
			blocks_as_size -= blocks_to_read;
		}
	}

	Ok(())
}

/// Serve a series of blocks that just contain 0's.
///
/// ## Errors
///
/// - If you are not running a 32 bit machine, and thus we cannot turn
///   a u32 into a usize.
/// - If we cannot send bytes over the `response_channel`.
async fn serve_zeroed_blocks(
	blocks_requested: u32,
	response_channel: &Sender<Bytes>,
) -> Result<(), CatBridgeError> {
	let mut blocks_as_size =
		usize::try_from(blocks_requested).map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?;

	while blocks_as_size > 0 {
		let blocks_to_read = std::cmp::min(blocks_as_size, SDIO_BLOCKS_PER_PACKET);
		let bytes_to_read = blocks_to_read * SDIO_BLOCK_SIZE;

		let zero_buff = BytesMut::zeroed(bytes_to_read);
		response_channel
			.send(zero_buff.freeze())
			.await
			.map_err(NetworkError::SendQueueFailure)?;
		blocks_as_size -= blocks_to_read;
	}

	Ok(())
}
