//! SDIO Protocol implementations live here.
//!
//! NOTE: this is not a traditional SDIO protocol that you may be familiar with
//! this is very specific to the nintendo's CAT-DEV environment. It is also
//! split over two TCP ports.
//!
//! The only part that "has a protocol" too is the control port. The data port
//! is literally just transferring files around.

pub mod message;
pub mod read;
pub mod write;

use crate::fsemul::sdio::errors::SDIOProtocolError;

/// The size of an SDIO Block we end up serving.
pub const SDIO_BLOCK_SIZE: usize = 0x200_usize;
/// The size of an SDIO Block we end up serving.
pub const SDIO_BLOCK_SIZE_AS_U32: u32 = 0x200_u32;

/// The types of packets that can be received by the SDIO Control Port.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SdioControlPacketType {
	/// A series of simple log messages, or control type messages.
	Message,
	/// Instruction to read on the data stream.
	Read,
	/// Instruction to write on the data stream.
	Write,
	/// TODO(mythra): confirm this is what it does.
	///
	/// Seems to be used in older firmwares to tell the server to
	/// 'start' the SDIO block channel.
	StartBlockChannel,
	/// TODO(mythra): confirm this is what it does.
	///
	/// Seems to be used in older firmwares to tell the server to
	/// 'start' the CTRL Character channel.
	StartControlListeningChannel,
}

impl From<SdioControlPacketType> for u8 {
	fn from(value: SdioControlPacketType) -> Self {
		match value {
			SdioControlPacketType::Message => 8,
			SdioControlPacketType::Read => 0,
			SdioControlPacketType::Write => 1,
			SdioControlPacketType::StartBlockChannel => 0xA,
			SdioControlPacketType::StartControlListeningChannel => 0xB,
		}
	}
}

impl TryFrom<u8> for SdioControlPacketType {
	type Error = SDIOProtocolError;

	fn try_from(value: u8) -> Result<Self, Self::Error> {
		match value {
			0 => Ok(Self::Read),
			1 => Ok(Self::Write),
			8 => Ok(Self::Message),
			0xA => Ok(Self::StartBlockChannel),
			0xB => Ok(Self::StartControlListeningChannel),
			_ => Err(SDIOProtocolError::UnknownPrintfPacketType(value)),
		}
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[test]
	pub fn roundtrip_control_packet_type() {
		for packet_ty in vec![
			SdioControlPacketType::Message,
			SdioControlPacketType::Read,
			SdioControlPacketType::Write,
		] {
			assert_eq!(
				Ok(packet_ty),
				SdioControlPacketType::try_from(u8::from(packet_ty)),
				"Round-tripped control packet type was not the same?"
			);
		}
	}
}
