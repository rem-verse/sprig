//! Definitions for the `ReadDirectory` packet type, and it's response types.
//!
//! This will return the file information for the next file present within a
//! directory. This does not recurse.

use crate::{
	errors::NetworkParseError,
	fsemul::pcfs::{errors::PCFSApiError, sata::proto::get_info_by_query::SataFDInfo},
};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::ffi::CStr;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// No more items in this directory! sorry!
pub const NO_MORE_ITEMS: u32 = 0xFFF0_FFFC;

/// A packet to get information about another file within a directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataReadFolderPacketBody {
	file_descriptor: i32,
}

impl SataReadFolderPacketBody {
	/// Create a new read directory packet body.
	#[must_use]
	pub const fn new(file_descriptor: i32) -> Self {
		Self { file_descriptor }
	}

	#[must_use]
	pub const fn file_descriptor(&self) -> i32 {
		self.file_descriptor
	}

	pub const fn set_file_descriptor(&mut self) -> i32 {
		self.file_descriptor
	}
}

impl From<&SataReadFolderPacketBody> for Bytes {
	fn from(value: &SataReadFolderPacketBody) -> Self {
		let mut buff = BytesMut::with_capacity(4);
		buff.put_i32(value.file_descriptor);
		buff.freeze()
	}
}

impl From<SataReadFolderPacketBody> for Bytes {
	fn from(value: SataReadFolderPacketBody) -> Self {
		Self::from(&value)
	}
}

impl TryFrom<Bytes> for SataReadFolderPacketBody {
	type Error = NetworkParseError;

	fn try_from(mut value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x4 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataReadDir",
				"Body",
				0x4,
				value.len(),
				value,
			));
		}
		if value.len() > 0x4 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataReadDir",
				value.slice(0x4..),
			));
		}

		let fd = value.get_i32();

		Ok(Self {
			file_descriptor: fd,
		})
	}
}

const SATA_READ_FOLDER_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[NamedField::new("fd")];

impl Structable for SataReadFolderPacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataReadDirPacketBody",
			Fields::Named(SATA_READ_FOLDER_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataReadFolderPacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_READ_FOLDER_PACKET_BODY_FIELDS,
			&[Valuable::as_value(&self.file_descriptor)],
		));
	}
}

#[derive(Clone, Debug, PartialEq, Eq, Valuable)]
pub struct DirectoryItemResponse {
	/// The underlying return code for this directory item.
	return_code: u32,
	/// The next bit of file information, if there are any items left in the
	/// directory.
	next_file_info: Option<(SataFDInfo, String)>,
}

impl DirectoryItemResponse {
	/// Create a Directory Item.
	///
	/// This assumes there was another item in the directory and we want to
	/// return it.
	///
	/// ## Errors
	///
	/// If the path is longer than 255 bytes.
	pub fn new_next(info: SataFDInfo, path: String) -> Result<Self, PCFSApiError> {
		if path.len() > 255 {
			return Err(PCFSApiError::PathTooLong(path));
		}

		Ok(Self {
			return_code: 0,
			next_file_info: Some((info, path)),
		})
	}

	/// Return that there's nothing left in the directory.
	#[must_use]
	pub const fn new_nothing_left() -> Self {
		Self {
			return_code: NO_MORE_ITEMS,
			next_file_info: None,
		}
	}

	/// Create a new directory item response given an error code.
	#[must_use]
	pub const fn new_error_code(rc: u32) -> Self {
		Self {
			return_code: rc,
			next_file_info: None,
		}
	}

	/// Get the underlying return code for this directory item.
	#[must_use]
	pub const fn return_code(&self) -> u32 {
		self.return_code
	}

	/// Return if this directory item was 'successfully' fetched.
	///
	/// This is really only useful when performing manual construction of the
	/// read directory packet. As when deserializing we will always return a
	/// proper error code if it's not successful.
	#[must_use]
	pub fn is_successful(&self) -> bool {
		[NO_MORE_ITEMS, 0].contains(&self.return_code)
	}

	/// Get the file that was returned as being next in the directory.
	#[must_use]
	pub const fn file_info(&self) -> Option<&(SataFDInfo, String)> {
		self.next_file_info.as_ref()
	}

	/// Consume the underlying packet, and just get the next file info if there
	/// is any.
	#[must_use]
	pub fn take_file_info(self) -> Option<(SataFDInfo, String)> {
		self.next_file_info
	}
}

impl From<&DirectoryItemResponse> for Bytes {
	fn from(value: &DirectoryItemResponse) -> Self {
		if let Some(nfi) = value.file_info() {
			let mut buff = BytesMut::with_capacity(0x158);
			buff.put_u32(value.return_code());
			buff.extend(Bytes::from(&nfi.0));
			let path_bytes = nfi.1.as_bytes();
			buff.extend(Bytes::from(Vec::from(path_bytes)));
			buff.extend(BytesMut::zeroed(256 - path_bytes.len()));
			buff.freeze()
		} else {
			let mut bytes = BytesMut::with_capacity(0x158);
			bytes.put_u32(value.return_code());
			bytes.extend([0; 0x154]);
			bytes.freeze()
		}
	}
}

impl From<DirectoryItemResponse> for Bytes {
	fn from(value: DirectoryItemResponse) -> Self {
		Self::from(&value)
	}
}

impl TryFrom<Bytes> for DirectoryItemResponse {
	type Error = NetworkParseError;

	fn try_from(mut value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 4 {
			return Err(NetworkParseError::NotEnoughData(
				"DirectoryItemResponse",
				4,
				value.len(),
				value,
			));
		}

		// Okay we've at least got an rc....
		let rc = value.get_u32();
		if rc != 0 && rc != NO_MORE_ITEMS {
			return Err(NetworkParseError::ErrorCode(rc));
		}
		if rc == NO_MORE_ITEMS {
			return Ok(DirectoryItemResponse::new_nothing_left());
		}

		// rc is now guaranteed to be 0... We should expect a full packet.
		if value.len() < 0x154 {
			return Err(NetworkParseError::NotEnoughData(
				"DirectoryItemResponse",
				0x154,
				value.len(),
				value,
			));
		}
		if value.len() > 0x154 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"DirectoryItemResponse",
				value.slice(0x154..),
			));
		}

		let fd_info = SataFDInfo::try_from(value.slice(..84))?;
		let path_bytes = value.slice(84..);
		let path =
			CStr::from_bytes_until_nul(&path_bytes).map_err(NetworkParseError::BadCString)?;

		Ok(Self {
			return_code: rc,
			next_file_info: Some((fd_info, path.to_str()?.to_owned())),
		})
	}
}
