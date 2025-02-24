//! Definitions, and handlers for the `ReadFile` packet type.
//!
//! This is what actively handles reading bytes out of a file. With either
//! FFIO, and Combined Send/Recv options being turned on/off.

use crate::{errors::NetworkParseError, fsemul::pcfs::sata::proto::MoveToFileLocation};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

#[cfg(feature = "servers")]
use crate::{
	errors::{CatBridgeError, NetworkError},
	fsemul::{
		HostFilesystem,
		pcfs::sata::proto::{SataPacketHeader, construct_sata_response},
	},
};
#[cfg(feature = "servers")]
use tokio::sync::mpsc::Sender;
#[cfg(feature = "servers")]
use tracing::debug;

#[cfg(feature = "servers")]
/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;
#[cfg(feature = "servers")]
/// This should be half of the total size of the buffer which should always be
/// more than enough.
const MAX_PACKET_LEN: usize = 32768_usize;

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

	/// Handle reading from a file that is already open.
	///
	/// ## Errors
	///
	/// If we cannot construct a sata response packet because our data to send
	/// was somehow too large (this should ideally never happen), or if we're
	/// running on a 16 bit system.
	#[cfg(feature = "servers")]
	pub async fn handle(
		&self,
		request_header: &SataPacketHeader,
		host_filesystem: &HostFilesystem,
		ffio_supported: bool,
		output_channel: &Sender<Bytes>,
	) -> Result<(), CatBridgeError> {
		if self.should_move {
			match self.move_to_pointer {
				MoveToFileLocation::Begin => {
					if host_filesystem.seek_file(self.handle, true).await.is_err() {
						debug!(
							packet.fd = self.handle,
							packet.typ = "PCFSSrvReadFile",
							"Failed to seek to beginning of file!",
						);

						return Self::construct_error(request_header, FS_ERROR, output_channel)
							.await;
					}
				}
				MoveToFileLocation::Current => {
					// Luckily to move to current, we don't need to move at all.
				}
				MoveToFileLocation::End => {
					if host_filesystem.seek_file(self.handle, false).await.is_err() {
						debug!(
							packet.fd = self.handle,
							packet.typ = "PCFSSrvReadFile",
							"Failed to seek to end of file of file!",
						);

						return Self::construct_error(request_header, FS_ERROR, output_channel)
							.await;
					}
				}
			}
		}

		let Some(file_size) = host_filesystem.file_length(self.handle).await else {
			debug!(
				packet.fd = self.handle,
				packet.typ = "PCFSSrvReadFile",
				"Failed to query length of file!",
			);

			return Self::construct_error(request_header, FS_ERROR, output_channel).await;
		};
		let Ok(Some(read_file)) = host_filesystem
			.read_file(
				self.handle,
				usize::try_from(self.block_size)
					.map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?
					* usize::try_from(self.block_count)
						.map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?,
			)
			.await
		else {
			debug!(
				packet.fd = self.handle,
				packet.typ = "PCFSSrvReadFile",
				"Failed to read bytes of from file!",
			);

			return Self::construct_error(request_header, FS_ERROR, output_channel).await;
		};

		if ffio_supported {
			let mut buff = BytesMut::with_capacity(read_file.len() + 0x24);
			// The header is normally just 'malloc'd and not cleared between
			// buffers. Luckily for us we can just zero it out, and it's easier than
			// actually dealing with whatever random bytes PCFSServer would normally
			// send.
			buff.extend_from_slice(&[0; 0x20]);
			buff.put_u32(u32::try_from(file_size).unwrap_or(u32::MAX));
			buff.extend(read_file);

			for chunk in buff.chunks(MAX_PACKET_LEN) {
				// WE have to copy this vec with chunks allocations, this is because a
				// vector has potential seperated allocations that are not linear in
				// memory.
				//
				// So they may not lay on the exact boundaries we want to chunk in. So
				// we have to copy the data to get them in the linear area we want.
				output_channel
					.send(Bytes::from(Vec::from(chunk)))
					.await
					.map_err(NetworkError::SendQueueFailure)?;
			}

			Ok(())
		} else {
			todo!("Implement non-FFIO support.")
		}
	}

	#[cfg(feature = "servers")]
	async fn construct_error(
		packet_header: &SataPacketHeader,
		error_code: u32,
		output_channel: &Sender<Bytes>,
	) -> Result<(), CatBridgeError> {
		let mut buff = BytesMut::with_capacity(8);
		buff.put_u32(error_code);
		output_channel
			.send(construct_sata_response(packet_header, 0, buff.freeze())?)
			.await
			.map_err(NetworkError::SendQueueFailure)?;

		Ok(())
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

#[cfg(test)]
mod unit_tests {
	#[cfg(feature = "servers")]
	use super::*;
	#[cfg(feature = "servers")]
	use crate::fsemul::host_filesystem::test_helpers::{
		create_temporary_host_filesystem, join_many,
	};
	#[cfg(feature = "servers")]
	use tokio::{fs::OpenOptions, sync::mpsc::channel};

	#[cfg(feature = "servers")]
	#[tokio::test]
	pub async fn simple_ffio_read_file_request() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;

		let base_dir = join_many(tempdir.path(), ["data", "slc", "to-query"]);
		tokio::fs::create_dir(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		tokio::fs::write(join_many(&base_dir, ["file.txt"]), vec![0; 2])
			.await
			.expect("Failed to write test file!");
		let mocked_header = SataPacketHeader {
			packet_data_len: 0,
			packet_id: 0,
			flags: 0,
			version: 0,
			timestamp_on_host: 0,
			pid_on_host: 0,
		};

		let mut open_options = OpenOptions::new();
		open_options.read(true).create(false).write(false);
		let fd = fs
			.open_file(open_options, &join_many(&base_dir, ["file.txt"]))
			.await
			.expect("Failed to open file!");

		let read_request = SataReadFilePacketBody {
			block_count: 4,
			block_size: 1,
			handle: fd,
			move_to_pointer: MoveToFileLocation::Begin,
			should_move: false,
		};

		let (sender, mut receiver) = channel(1);

		read_request
			.handle(&mocked_header, &fs, true, &sender)
			.await
			.expect("Failed to handle read request!");
		let response = receiver
			.recv()
			.await
			.expect("Function edited without sending!");
		let mut expected_response = BytesMut::new();
		// Header
		expected_response.extend_from_slice(&[0; 0x20]);
		// File length.
		expected_response.extend_from_slice(&2_u32.to_be_bytes());
		// File data, and padding.
		expected_response.extend_from_slice(&[0x00, 0x00, 0xCD, 0xCD]);
		assert_eq!(response, expected_response.freeze());
	}
}
