//! Handle SDIO Read requests coming into our server.

use crate::{
	errors::{CatBridgeError, FSError},
	fsemul::{
		HostFilesystem,
		dlf::DiskLayoutFile,
		sdio::{
			data_stream::DataStream,
			errors::{SDIONetworkError, SDIOProtocolError},
			proto::{SDIO_BLOCK_SIZE, read::SdioControlReadRequest},
			server::SDIO_DATA_STREAMS,
		},
	},
	net::{
		additions::StreamID,
		server::requestable::{Body, State},
	},
};
use bytes::{Bytes, BytesMut};
use std::path::PathBuf;
use tokio::{
	fs::{File, read as fs_read},
	io::{AsyncReadExt, AsyncSeekExt, SeekFrom},
};
use tracing::{info, warn};

/// Handle a read request.
///
/// This will not respond on the "control" channel at all, but instead will
/// send bytes out over the "data" channel.
pub(super) async fn handle_read_request(
	State(fs): State<HostFilesystem>,
	stream_id: StreamID,
	Body(request): Body<SdioControlReadRequest>,
) -> Result<(), CatBridgeError> {
	let Some(data_stream) = SDIO_DATA_STREAMS.get(&stream_id.to_raw()) else {
		return Err(SDIONetworkError::DataStreamMissing(stream_id.to_raw()).into());
	};

	let address_to_read = request.lba();
	if address_to_read == 0xFFFF_0000 {
		info!("Requested special ppc_boot.bsf file address");
		let ppc_boot = fs.boot1_sytstem_path().await?;
		return serve_padded_file_sdio(
			&ppc_boot,
			SeekFrom::Start(0),
			request.blocks(),
			&data_stream,
		)
		.await;
	// Probably another special address like diskid or something.
	} else if address_to_read == 0x00F9_0000 {
		info!("Unknown special large address... serving 0 blocks");
		return serve_zeroed_blocks(request.blocks(), &data_stream).await;
	}

	let dlf = DiskLayoutFile::try_from(Bytes::from(
		fs_read(fs.ppc_boot_dlf_path().await?)
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

	if let Some((path, offset)) = dlf
		.get_path_and_offset_for_file(u128::from(address_to_read))
		.await
	{
		info!(
			sdio.blocks = request.blocks(),
			sdio.path = %path.display(),
			sdio.path_offset = format!("{:06x}", offset),
			"Serving known file over SDIO",
		);
		serve_padded_file_sdio(
			path,
			SeekFrom::Start(offset),
			request.blocks(),
			&data_stream,
		)
		.await
	} else {
		warn!(
			sdio.address = format!("{:02x}", address_to_read),
			sdio.blocks = request.blocks(),
			"Serving unknown address over SDIO",
		);
		serve_zeroed_blocks(request.blocks(), &data_stream).await
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
/// - If we cannot send content over the `data_channel`.
async fn serve_padded_file_sdio(
	path: &PathBuf,
	offset: SeekFrom,
	blocks_requested: u32,
	data_channel: &DataStream,
) -> Result<(), CatBridgeError> {
	let mut fd = File::open(path).await.map_err(FSError::IO)?;
	fd.seek(offset).await.map_err(FSError::IO)?;

	let blocks_left_to_serve =
		usize::try_from(blocks_requested).map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?;
	let total_byte_size = blocks_left_to_serve * SDIO_BLOCK_SIZE;
	let mut file_buff = BytesMut::zeroed(total_byte_size);
	let read_bytes = fd.read(&mut file_buff).await.map_err(FSError::IO)?;
	if read_bytes < total_byte_size {
		let padding = BytesMut::zeroed(total_byte_size - read_bytes);
		file_buff.extend(padding);
	}
	data_channel.send(file_buff.freeze()).await?;

	Ok(())
}

/// Serve a series of blocks that just contain 0's.
///
/// ## Errors
///
/// - If you are not running a 32 bit machine, and thus we cannot turn
///   a u32 into a usize.
/// - If we cannot send bytes over the `data_channel`.
async fn serve_zeroed_blocks(
	blocks_requested: u32,
	data_channel: &DataStream,
) -> Result<(), CatBridgeError> {
	let blocks_as_size =
		usize::try_from(blocks_requested).map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?;

	data_channel
		.send(BytesMut::zeroed(blocks_as_size * SDIO_BLOCK_SIZE).freeze())
		.await?;

	Ok(())
}
