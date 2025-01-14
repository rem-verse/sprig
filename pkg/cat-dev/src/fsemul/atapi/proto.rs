//! Protocols related to ATAPI emulation.
//!
//! This protocol much like SDIO though, ***IS NOT STANDARD***. I'm sorry if
//! you got a search request for ATAPI, and were looking for actual real ATAPI
//! code. Not this weird nintendo variant.

use bytes::{Bytes, BytesMut};
use tokio::io::Error as IoError;
use tokio_util::codec::{Decoder, Encoder};

/// A codec that chunks a stream into ATAPI packets.
///
/// All packets towards ATAPI are 12 bytes, but going out they can be
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
