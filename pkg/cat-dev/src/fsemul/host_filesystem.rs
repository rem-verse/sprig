//! A representation of the filesystem folder we end up serving a cat-dev
//! client.

use crate::{
	errors::{CatBridgeError, FSError, NetworkError},
	fsemul::sdio::proto::SdioControlReadRequest,
};
use bytes::{Bytes, BytesMut};
use std::path::PathBuf;
use tokio::{
	fs::File,
	io::{AsyncReadExt, BufReader},
	sync::mpsc::Sender,
};
use tracing::{field::valuable, info, warn};

/// The size of an SDIO Block we end up serving.
const SDIO_BLOCK_SIZE: usize = 512_usize;
/// The size of a single TCP packet we should end up serving.
const SDIO_TCP_PACKET_SIZE: usize = 65536_usize;
/// The amount of blocks that can fit within a single packet.
const SDIO_BLOCKS_PER_PACKET: usize = SDIO_TCP_PACKET_SIZE / SDIO_BLOCK_SIZE;

/// A pointer to a directory that will allow reading/writing over SDIO for
/// a cat-dev.
#[derive(Debug, PartialEq, Eq)]
pub struct HostFilesystem {
	/// The path to the base data directory to serve a filesystem out of.
	cafe_sdk_path: PathBuf,
}

impl HostFilesystem {
	/// Create a filesystem from a root cafe dir.
	///
	/// ## Errors
	///
	/// If the Cafe SDK directory is corrupt, or can't be found.
	pub fn from_cafe_dir(cafe_dir: Option<PathBuf>) -> Result<Self, FSError> {
		let Some(cafe_base_dir) = cafe_dir.or_else(Self::default_cafe_directory) else {
			return Err(FSError::CantFindCafeSdkPath);
		};

		if !cafe_base_dir
			.join("data")
			.join("mlc")
			.join("sys")
			.join("title")
			.join("00050030")
			.join("1001000A")
			.join("code")
			.join("app.xml")
			.exists() || !cafe_base_dir
			.join("data")
			.join("mlc")
			.join("sys")
			.join("title")
			.join("00050030")
			.join("1001010A")
			.join("code")
			.join("app.xml")
			.exists() || !cafe_base_dir
			.join("data")
			.join("mlc")
			.join("sys")
			.join("title")
			.join("00050030")
			.join("1001020A")
			.join("code")
			.join("app.xml")
			.exists()
		{
			return Err(FSError::CafeSdkPathCorrupt);
		}

		// Can't generate a `fw.img` file for now :(
		if !cafe_base_dir
			.join("data")
			.join("slc")
			.join("sys")
			.join("title")
			.join("00050010")
			.join("1000400A")
			.join("code")
			.join("fw.img")
			.exists()
		{
			return Err(FSError::CafeSdkPathCorrupt);
		}

		Ok(Self {
			cafe_sdk_path: cafe_base_dir,
		})
	}

	/// Serve a file from an SDIO read request.
	///
	/// TODO(mythra): actually figure out how address maps to actual files.
	///
	/// ## Errors
	///
	/// - If the device requests an unknown address to read files from.
	/// - If we cannot read the file from the disk.
	/// - If we cannot serve the file to a client.
	pub async fn serve_sdio_file(
		&self,
		read_request: SdioControlReadRequest,
		sender: Sender<Bytes>,
	) -> Result<(), CatBridgeError> {
		match read_request.lba() {
			0x80 => {
				// all 0's
				info!(
				  sdio.host_path = %self.cafe_sdk_path.display(),
				  sdio.request = valuable(&read_request),
				  "Requested known, but unmapped part of the disk, serving 0's for blocks.",
				);

				Self::serve_zeroed_blocks(read_request.blocks(), sender).await
			}
			0x7F_FF80 => {
				let requested_path = self
					.cafe_sdk_path
					.join("temp")
					.join("mythra")
					.join("caferun")
					.join("ppc.bsf");
				info!(
				  sdio.host_path = %self.cafe_sdk_path.display(),
				  sdio.request = valuable(&read_request),
				  sdio.serving = %requested_path.display(),
				  "Serving known file over PCFS",
				);

				Self::serve_padded_file_sdio(requested_path, read_request.blocks(), sender).await
			}
			0x480 => {
				let requested_path = self
					.cafe_sdk_path
					.join("data")
					.join("slc")
					.join("sys")
					.join("title")
					.join("00050010")
					.join("1000400A")
					.join("code")
					.join("fw.img");
				info!(
				  sdio.host_path = %self.cafe_sdk_path.display(),
				  sdio.request = valuable(&read_request),
				  sdio.serving = %requested_path.display(),
				  "Serving known file over PCFS",
				);

				Self::serve_padded_file_sdio(requested_path, read_request.blocks(), sender).await
			}
			0x7C80 => {
				// all 0's
				info!(
				  sdio.host_path = %self.cafe_sdk_path.display(),
				  sdio.request = valuable(&read_request),
				  "Requested known, but unmapped part of the disk, serving 0's for blocks.",
				);

				Self::serve_zeroed_blocks(read_request.blocks(), sender).await
			}
			_ => {
				warn!(
				  sdio.host_path = %self.cafe_sdk_path.display(),
				  sdio.request = valuable(&read_request),
				  "Unknown LBA for read-request!",
				);

				Err(NetworkError::UnknownPCFSServerAddress(read_request.lba()).into())
			}
		}
	}

	#[allow(
    // Not actually unreachable unless on unsupported OS.
    unreachable_code,
  )]
	#[must_use]
	pub fn default_cafe_directory() -> Option<PathBuf> {
		#[cfg(target_os = "windows")]
		{
			return Some(PathBuf::from(r"C:\cafe_sdk"));
		}

		#[cfg(any(
			target_os = "linux",
			target_os = "freebsd",
			target_os = "openbsd",
			target_os = "netbsd",
			target_os = "macos"
		))]
		{
			return Some(PathBuf::from("/opt/cafe_sdk"));
		}

		None
	}

	/// Serve a file over a tcp stream to SDIO.
	async fn serve_padded_file_sdio(
		path: PathBuf,
		blocks_requested: u32,
		sender: Sender<Bytes>,
	) -> Result<(), CatBridgeError> {
		let mut fd = File::open(path).await.map_err(FSError::IOError)?;

		let mut blocks_as_size = usize::try_from(blocks_requested)
			.map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?;
		// Small enough, ready to just be read one-shot.
		if blocks_as_size <= SDIO_BLOCKS_PER_PACKET {
			let mut file_buff = BytesMut::with_capacity(blocks_as_size * SDIO_BLOCK_SIZE);
			let read_bytes = fd
				.read_buf(&mut file_buff)
				.await
				.map_err(FSError::IOError)?;
			if read_bytes < blocks_as_size * SDIO_BLOCK_SIZE {
				let padding = BytesMut::zeroed((blocks_as_size * SDIO_BLOCK_SIZE) - read_bytes);
				file_buff.extend(padding);
			}
			sender
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
					let read_bytes = reader
						.read_buf(&mut file_buff)
						.await
						.map_err(FSError::IOError)?;
					if read_bytes == 0 {
						exhausted_file = true;
					}
					read_bytes
				};

				if read_bytes < bytes_to_read {
					let padding = BytesMut::zeroed(bytes_to_read - read_bytes);
					file_buff.extend(padding);
				}

				sender
					.send(file_buff.freeze())
					.await
					.map_err(NetworkError::SendQueueFailure)?;
				blocks_as_size -= blocks_to_read;
			}
		}

		Ok(())
	}

	/// Serve totally empty zero'd blocks.
	async fn serve_zeroed_blocks(
		blocks_requested: u32,
		sender: Sender<Bytes>,
	) -> Result<(), CatBridgeError> {
		let mut blocks_as_size = usize::try_from(blocks_requested)
			.map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?;

		while blocks_as_size > 0 {
			let blocks_to_read = std::cmp::min(blocks_as_size, SDIO_BLOCKS_PER_PACKET);
			let bytes_to_read = blocks_to_read * SDIO_BLOCK_SIZE;

			let zero_buff = BytesMut::zeroed(bytes_to_read);
			sender
				.send(zero_buff.freeze())
				.await
				.map_err(NetworkError::SendQueueFailure)?;
			blocks_as_size -= blocks_to_read;
		}

		Ok(())
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	fn only_accepts_send_sync<T: Send + Sync>(_opt: Option<T>) {}

	#[test]
	pub fn is_send_sync() {
		only_accepts_send_sync::<HostFilesystem>(None);
	}
}
