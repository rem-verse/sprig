//! Definitions for the `ReadFile` packet type, and it's response types.
//!
//! This is what actively handles reading bytes out of a file. With either
//! FFIO, and Combined Send/Recv options being turned on/off.

use crate::{errors::NetworkParseError, fsemul::pcfs::sata::proto::MoveToFileLocation};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// A packet to read the contents of an already open file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataReadFilePacketBody {
	block_count: u32,
	block_size: u32,
	handle: i32,
	move_to_pointer: MoveToFileLocation,
	should_move: bool,
}

impl SataReadFilePacketBody {
	/// Create a new read file packet.
	#[must_use]
	pub const fn new(
		block_count: u32,
		block_size: u32,
		file_descriptor: i32,
		move_to: Option<MoveToFileLocation>,
	) -> Self {
		Self {
			block_count,
			block_size,
			handle: file_descriptor,
			move_to_pointer: if let Some(mt) = move_to {
				mt
			} else {
				MoveToFileLocation::Begin
			},
			should_move: move_to.is_some(),
		}
	}

	#[must_use]
	pub const fn block_count(&self) -> u32 {
		self.block_count
	}

	pub const fn set_block_count(&mut self, new_count: u32) {
		self.block_count = new_count;
	}

	#[must_use]
	pub const fn block_size(&self) -> u32 {
		self.block_size
	}

	pub const fn set_block_size(&mut self, new_size: u32) {
		self.block_size = new_size;
	}

	#[must_use]
	pub const fn file_descriptor(&self) -> i32 {
		self.handle
	}

	pub const fn set_file_descriptor(&mut self, new_fd: i32) {
		self.handle = new_fd;
	}

	#[must_use]
	pub const fn move_to_pointer(&self) -> MoveToFileLocation {
		self.move_to_pointer
	}

	pub const fn set_move_to(&mut self, new_move: Option<MoveToFileLocation>) {
		if let Some(nm) = new_move {
			self.move_to_pointer = nm;
			self.should_move = true;
		} else {
			self.move_to_pointer = MoveToFileLocation::Begin;
			self.should_move = false;
		}
	}

	#[must_use]
	pub const fn should_move(&self) -> bool {
		self.should_move
	}
}

impl From<&SataReadFilePacketBody> for Bytes {
	fn from(value: &SataReadFilePacketBody) -> Self {
		let mut buff = BytesMut::with_capacity(20);

		buff.put_u32(value.block_count);
		buff.put_u32(value.block_size);
		buff.put_i32(value.handle);
		buff.put_u32(u32::from(value.move_to_pointer));
		// True is 1, False is 0
		buff.put_u32(u32::from(value.should_move));

		buff.freeze()
	}
}

impl From<SataReadFilePacketBody> for Bytes {
	fn from(value: SataReadFilePacketBody) -> Self {
		Self::from(&value)
	}
}

impl TryFrom<Bytes> for SataReadFilePacketBody {
	type Error = NetworkParseError;

	fn try_from(mut value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 20 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataReadFile",
				"Body",
				20,
				value.len(),
				value,
			));
		}
		if value.len() > 20 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataReadFile",
				value.slice(20..),
			));
		}

		let block_count = value.get_u32();
		let block_length = value.get_u32();
		let handle = value.get_i32();
		let move_to_ptr = value.get_u32();
		let should_move = value.get_u32();

		Ok(Self {
			block_count,
			block_size: block_length,
			handle,
			move_to_pointer: MoveToFileLocation::try_from(move_to_ptr)?,
			should_move: (should_move & 1) != 0,
		})
	}
}

const SATA_READ_FILE_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("block_count"),
	NamedField::new("block_size"),
	NamedField::new("handle"),
	NamedField::new("move_to_pointer"),
	NamedField::new("should_move"),
];

impl Structable for SataReadFilePacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataReadFilePacketBody",
			Fields::Named(SATA_READ_FILE_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataReadFilePacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_READ_FILE_PACKET_BODY_FIELDS,
			&[
				Valuable::as_value(&self.block_count),
				Valuable::as_value(&self.block_size),
				Valuable::as_value(&self.handle),
				Valuable::as_value(&self.move_to_pointer),
				Valuable::as_value(&self.should_move),
			],
		));
	}
}
