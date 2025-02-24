//! Protocols related to ATAPI emulation.
//!
//! This protocol much like SDIO though, ***IS NOT STANDARD***. I'm sorry if
//! you got a search request for ATAPI, and were looking for actual real ATAPI
//! code. Not this weird nintendo variant.

use bytes::{Bytes, BytesMut};
use tokio::io::Error as IoError;
use tokio_util::codec::{Decoder, Encoder};

#[cfg(feature = "clients")]
use crate::fsemul::atapi::errors::ATAPIProtocolError;

/// A codec that chunks a stream into ATAPI packets.
///
/// All packets towards ATAPI are 12 bytes, but going out they can be
/// any size.
#[cfg(feature = "servers")]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ChunkATAPIEmulatorCodec;

#[cfg(feature = "servers")]
impl Decoder for ChunkATAPIEmulatorCodec {
	type Item = BytesMut;
	type Error = IoError;

	fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
		if src.len() < 12 {
			Ok(None)
		} else {
			Ok(Some(src.split_to(12)))
		}
	}
}

#[cfg(feature = "servers")]
impl Encoder<Bytes> for ChunkATAPIEmulatorCodec {
	type Error = IoError;

	fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> Result<(), Self::Error> {
		dst.reserve(item.len());
		dst.extend(item);
		Ok(())
	}
}

#[cfg(feature = "clients")]
/// A codec that chunks a stream into ATAPI packets from the client
/// perspective.
///
/// All packets towards ATAPI are 12 bytes, but going out they can be
/// any size.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ClientChunkedATAPIEmulatorCode;

#[cfg(feature = "clients")]
impl Decoder for ClientChunkedATAPIEmulatorCode {
	type Item = BytesMut;
	type Error = IoError;

	fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
		// Data coming from the server can be any size.
		//
		// TODO(mythra): figure out how to actually do proper length checking here.
		Ok(Some(src.split()))
	}
}

#[cfg(feature = "clients")]
impl Encoder<Bytes> for ClientChunkedATAPIEmulatorCode {
	type Error = IoError;

	fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> Result<(), Self::Error> {
		if item.len() < 12 {
			Err(IoError::other(
				ATAPIProtocolError::InvalidClientRequestLength(item),
			))
		} else {
			dst.reserve(12);
			dst.extend(item);
			Ok(())
		}
	}
}

#[cfg(feature = "servers")]
pub mod read_packet_temp_will_break {
	//! Functions related to handling reads from the ATAPI interface.
	//!
	//! These are the bits that handle all of the translation from an ATAPI packet
	//! to a real file being sent back. They mostly are just wrappers around
	//! [`HostFilesystem`] calls, being wrapped in ATAPI protocols.

	use crate::{
		errors::{CatBridgeError, FSError, NetworkError},
		fsemul::{HostFilesystem, atapi::proto::ChunkATAPIEmulatorCodec, dlf::DiskLayoutFile},
	};
	use bytes::{Bytes, BytesMut};
	use futures::{SinkExt, stream::SplitSink};
	use tokio::{
		fs::{File, read as fs_read},
		io::{AsyncReadExt, AsyncSeekExt, SeekFrom},
		net::TcpStream,
	};
	use tokio_util::codec::Framed;
	use tracing::debug;

	/// Handle reading a file from ATAPI.
	///
	/// ## Errors
	///
	/// - If the total amount of data to read is too large to read.
	/// - If we cannot load & parse the DLF from the Host Path.
	/// - If we cannot send the buffer out over the network.
	pub async fn handle_read_dlf(
		packet: Bytes,
		host_filesystem: &HostFilesystem,
		output: &mut SplitSink<Framed<TcpStream, ChunkATAPIEmulatorCodec>, Bytes>,
	) -> Result<(), CatBridgeError> {
		let read_address = u128::from(u32::from_be_bytes([
			packet[0x4],
			packet[0x5],
			packet[0x6],
			packet[0x7],
		])) << 11_u128;
		let read_length = u128::from(u32::from_be_bytes([
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

		if let Some((path, offset)) = dlf.get_path_and_offset_for_file(read_address).await {
			let metadata = path.metadata().map_err(FSError::from)?;
			let file_size_bytes = usize::try_from(metadata.len() - offset).unwrap_or(usize::MAX);
			// Read the file contents...
			let mut handle = File::open(&path).await.map_err(FSError::from)?;
			handle
				.seek(SeekFrom::Start(offset))
				.await
				.map_err(FSError::from)?;
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
}

// KNOWN PACKET HEADERS
//
//  - [0x3]
//    - send 32 bytes of various describes, not quite sure
//  - [0x12]
//    - sends 96 bytes, seems to have some random spattering of fields
//  - [0xCF, 0x80] -> Triggers Events in FSEmul, probably just call "EVentTrigger"
//  - [0xF0] -> send back 4, 0x0 bytes
//  - [0xF1]
//    - [0xF1, 0x00]
//      - seems to literally send 32 bytes of random data.... cool
//    - [0xF1, 0x02]
//      - seems to literally send 32 bytes of random data.... cool
//    - [0xF1, 0x01] || [0xF1, 0x03]
//      - seems to do nothing on hthe network
//  - [0xF2]
//    - [0xF2, 0x00]
//    - [0xF2, 0x01]
//    - [0xF2, 0x02]
//    - [0xF2, 0x03]
//      -> ??? calls a dynamically allocated thing
//    - [0xF2, 0x06]
//    - [0xF2, 0x07]
//      -> ??? calls a dynamically allocated thing
//      -> for f207 looks like we don't send _anything_ back by default
//      -> seems some paths check for Dvdroot, so maybe dvd stuff?
//  - [0xF3]
//    - [0xF3, 0x0] -> seems to actually be real "read file"
//      - not quite sure exactly how to parse this yet, but these are examples:
//        - first 4 bytes are "packet id"
//        - second 4 bytes are "read address" (calculate by: cast to u128 `<< 11`)
//        - last 4 bytes are "read length" (calculate by: cast to u128 `<< 11`)
//      - logs seem to indicate we:
//         1. read a dlf file (dlf file is populated in cafe-tmp how get?)
//         2. use that to get a max read address
//         3. read from the file
//         4. then pad
//        see logs below:
//          - `CSataProcessor::could not get lead out from dlffileobj {error code}`
//          - `CSataProcessor::requested read address 0x%I64x is out of bounds.`
//          - `CSataProcessor::could not read from file`
//          - `CSataProcessor::padding`
//          - `CSataProcessor::error writing to MION port`
//          - `CSataProcessor::wrote %d bytes`
//    - [0xF3, 0x1] -> send back "PC SATA EMUL" + 20 0's
//    - [0xF3, 0x2] || [0xF3, 0x3] -> send back 32 0's - seems this is always set to 0. maybe a kind of ping?
//  - [0xF5]
//    - send 32 0's
//  - [0xF6]
//    - second byte &3 != 0 -> doesn't send reply
//    - if not send what looks to be `1` encoded as 4 bytes
//  - [0xF7]
//    - send 32 0's
