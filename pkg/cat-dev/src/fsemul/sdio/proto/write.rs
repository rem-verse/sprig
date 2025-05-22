//! An SDIO 'write' request to write data somewhere on the disk.

use crate::fsemul::sdio::{
	errors::{SDIOAPIError, SDIOProtocolError},
	proto::SDIO_BLOCK_SIZE_AS_U32,
};
use bytes::{BufMut, Bytes, BytesMut};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// Handle a Write Request coming over the SDIO Control port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdioControlWriteRequest {
	lba: u32,
	blocks: u32,
	channel: u32,
}

impl SdioControlWriteRequest {
	/// Create a new SDIO Write Request to head to the `CONTROL` port.
	///
	/// This assumes you're passing in the LBA as it appears in the DLF file,
	/// (e.g. divisble by [`SDIO_BLOCK_SIZE_AS_U32`]). If you want the raw LBA
	/// you can call [`Self::new_with_raw_lba`].
	///
	/// ## Errors
	///
	/// - If the LBA address is not a possible block address (must be divisble by
	///   [`SDIO_BLOCK_SIZE_AS_U32`]).
	/// - If the channel's first byte is greater than or equal to `0xC` when
	///   encoded to little endian.
	pub fn new(lba: u32, blocks: u32, channel: u32) -> Result<Self, SDIOAPIError> {
		if lba < SDIO_BLOCK_SIZE_AS_U32 || lba % SDIO_BLOCK_SIZE_AS_U32 != 0 {
			return Err(SDIOAPIError::InvalidLBA(lba));
		}
		let as_bytes = channel.to_le_bytes();
		if as_bytes[0] >= 0xC {
			return Err(SDIOAPIError::InvalidChannel(as_bytes[0], channel));
		}

		Ok(Self {
			lba: lba / SDIO_BLOCK_SIZE_AS_U32,
			blocks,
			channel,
		})
	}

	/// Create a new SDIO Write Request to head to the `CONTROL` port.
	///
	/// This assumes you're passing in the LBA as it appears on the network,
	/// (e.g. not the number being divisble by 512).
	///
	/// ## Errors
	///
	/// - If the channel's first byte is greater than or equal to `0xC` when
	///   encoded to little endian.
	pub fn new_with_raw_lba(raw_lba: u32, blocks: u32, channel: u32) -> Result<Self, SDIOAPIError> {
		let as_bytes = channel.to_le_bytes();
		if as_bytes[0] >= 0xC {
			return Err(SDIOAPIError::InvalidChannel(as_bytes[0], channel));
		}

		Ok(Self {
			lba: raw_lba,
			blocks,
			channel,
		})
	}

	/// Get the address to write to.
	#[must_use]
	pub const fn lba(&self) -> u32 {
		self.lba * SDIO_BLOCK_SIZE_AS_U32
	}

	/// Set the "raw" address to write to, this is not in the same form as
	/// `write_request.lba()`.
	///
	/// This is if we took the LBA returned by the LBA block method and divided
	/// it by block size.
	pub fn set_raw_lba(&mut self, new_lba: u32) {
		self.lba = new_lba;
	}

	/// Set the address to write to.
	///
	/// ## Errors
	///
	/// - If the LBA address is not a possible block address (must be divisble by
	///   [`SDIO_BLOCK_SIZE_AS_U32`]).
	pub fn set_lba(&mut self, new_lba: u32) -> Result<(), SDIOAPIError> {
		if new_lba < SDIO_BLOCK_SIZE_AS_U32 || new_lba % SDIO_BLOCK_SIZE_AS_U32 != 0 {
			return Err(SDIOAPIError::InvalidLBA(new_lba));
		}

		self.lba = new_lba / SDIO_BLOCK_SIZE_AS_U32;
		Ok(())
	}

	/// Get the amount of blocks to read.
	#[must_use]
	pub const fn blocks(&self) -> u32 {
		self.blocks
	}

	/// Set the amount of blocks to write to SDIO.
	pub fn set_blocks(&mut self, new_blocks: u32) {
		self.blocks = new_blocks;
	}

	/// Get the channel to read from.
	#[must_use]
	pub const fn channel(&self) -> u32 {
		self.channel
	}

	/// Set the channel this write request is on.
	///
	/// ## Errors
	///
	/// - If the channel's first byte is greater than or equal to `0xC` when
	///   encoded to little endian.
	pub fn set_channel(&mut self, new_channel: u32) -> Result<(), SDIOAPIError> {
		let as_bytes = new_channel.to_le_bytes();
		if as_bytes[0] >= 0xC {
			return Err(SDIOAPIError::InvalidChannel(as_bytes[0], new_channel));
		}

		self.channel = new_channel;
		Ok(())
	}
}

impl TryFrom<Bytes> for SdioControlWriteRequest {
	type Error = SDIOProtocolError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if value.len() != 512 {
			return Err(SDIOProtocolError::PrintfInvalidSize(value.len()));
		}
		if value[0] != 1 {
			return Err(SDIOProtocolError::UnknownPrintfPacketType(value[0]));
		}

		let lba = u32::from_le_bytes([value[4], value[5], value[6], value[7]]);
		let blocks = u32::from_le_bytes([value[8], value[9], value[10], value[11]]);
		let channel = u32::from_le_bytes([value[12], value[13], value[14], value[15]]);
		if value[12] >= 0xC {
			return Err(SDIOProtocolError::PrintfInvalidChannel(value[12], channel));
		}

		Ok(Self {
			lba,
			blocks,
			channel,
		})
	}
}

impl From<&SdioControlWriteRequest> for Bytes {
	fn from(value: &SdioControlWriteRequest) -> Self {
		let mut serialized = BytesMut::with_capacity(512);
		serialized.put_u32_le(1);
		serialized.put_u32_le(value.lba);
		serialized.put_u32_le(value.blocks);
		serialized.put_u32_le(value.channel);
		serialized.extend_from_slice(&[0; 0x1F0]);
		serialized.freeze()
	}
}

impl From<SdioControlWriteRequest> for Bytes {
	fn from(value: SdioControlWriteRequest) -> Self {
		Self::from(&value)
	}
}

const CONTROL_WRITE_REQUEST_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("raw_lba"),
	NamedField::new("blocks"),
	NamedField::new("channel"),
];

impl Structable for SdioControlWriteRequest {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SdioControlReadRequest",
			Fields::Named(CONTROL_WRITE_REQUEST_FIELDS),
		)
	}
}

impl Valuable for SdioControlWriteRequest {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			CONTROL_WRITE_REQUEST_FIELDS,
			&[
				Valuable::as_value(&self.lba),
				Valuable::as_value(&self.blocks),
				Valuable::as_value(&self.channel),
			],
		));
	}
}

#[cfg(test)]
mod unit_tests {
	// TODO(mythra): get a real life write request.
}
