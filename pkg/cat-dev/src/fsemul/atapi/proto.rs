//! Protocols related to ATAPI emulation.
//!
//! This protocol much like SDIO though, ***IS NOT STANDARD***. I'm sorry if
//! you got a search request for ATAPI, and were looking for actual real ATAPI
//! code. Not this weird nintendo variant.

// TODO(mythra): everything in here has a chance of being wrong, and is probably wrong.

use bytes::{Bytes, BytesMut};
use tokio::io::Error as IoError;
use tokio_util::codec::{Decoder, Encoder};

/// A codec that chunks a stream into ATAPI packets.
///
/// All packets towards ATAPI seem to be 12 bytes, but going out they can be
/// any size.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ChunkATAPIEmulatorCodec;

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

impl Encoder<Bytes> for ChunkATAPIEmulatorCodec {
	type Error = IoError;

	fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> Result<(), Self::Error> {
		dst.reserve(item.len());
		dst.extend(item);
		Ok(())
	}
}
