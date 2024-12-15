//! Functions related to handling reads from the ATAPI interface.
//!
//! These are the bits that handle all of the translation from an ATAPI packet
//! to a real file being sent back. They mostly are just wrappers around
//! [`HostFilesystem`] calls, being wrapped in ATAPI protocols.

use crate::{
	errors::{CatBridgeError, FSError, NetworkError},
	fsemul::{atapi::ChunkATAPIEmulatorCodec, dlf::DiskLayoutFile, HostFilesystem},
};
use bytes::{Bytes, BytesMut};
use futures::{stream::SplitSink, SinkExt};
use tokio::{
	fs::{read as fs_read, File},
	io::AsyncReadExt,
	net::TcpStream,
};
use tokio_util::codec::Framed;
use tracing::debug;

/// Handle reading a file from ATAPI.
pub async fn handle_read_dlf(
	packet: Bytes,
	host_filesystem: &HostFilesystem,
	output: &mut SplitSink<Framed<TcpStream, ChunkATAPIEmulatorCodec>, Bytes>,
) -> Result<(), CatBridgeError> {
	let read_address = u128::from(u32::from_le_bytes([
		packet[0x4],
		packet[0x5],
		packet[0x6],
		packet[0x7],
	])) << 11_u128;
	let read_length = u128::from(u32::from_le_bytes([
		packet[0x8],
		packet[0x9],
		packet[0xA],
		packet[0xB],
	])) << 11_u128;
	let rl_as_usize = usize::try_from(read_length)
		.map_err(|_| NetworkError::RequestedSizeTooLarge(read_length))?;

	debug!(
		atapi.packet_type = "read_address",
		atapi.read_address.address = %read_address,
		atapi.read_address.length = %read_length,
		"Handling atapi read request!"
	);

	let bytes_of_dlf = fs_read(host_filesystem.ppc_boot_dlf_path().await?)
		.await
		.map_err(FSError::from)?;
	let dlf = DiskLayoutFile::try_from(Bytes::from(bytes_of_dlf))?;

	if let Some(path) = dlf.get_path_for_address(read_address) {
		let metadata = path.metadata().map_err(FSError::from)?;
		let file_size_bytes = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
		// Read the file contents...
		let mut handle = File::open(&path).await.map_err(FSError::from)?;
		let mut buff = BytesMut::zeroed(std::cmp::min(file_size_bytes, rl_as_usize));
		handle.read_exact(&mut buff).await.map_err(FSError::from)?;
		std::mem::drop(handle);
		// Pad if necessary...
		if file_size_bytes < rl_as_usize {
			buff.reserve(rl_as_usize - file_size_bytes);
			buff.extend(BytesMut::zeroed(rl_as_usize - file_size_bytes));
		}
		// Send!
		Ok(output
			.send(buff.freeze())
			.await
			.map_err(NetworkError::from)?)
	} else {
		Ok(output
			.send(BytesMut::zeroed(rl_as_usize).freeze())
			.await
			.map_err(NetworkError::from)?)
	}
}
