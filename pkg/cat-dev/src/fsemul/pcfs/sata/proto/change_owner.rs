//! Definitions, and handlers for the `ChangeOwner` packet type.
//!
//! For some reason this always responds with an error. Rather than setting an
//! actual `uid`/`gid`. This is because windows doesn't have the concept of a
//! uid/gid.

use crate::{errors::NetworkParseError, fsemul::pcfs::errors::PCFSApiError};
use bytes::{BufMut, Bytes, BytesMut};
use std::ffi::CStr;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

#[cfg(feature = "servers")]
use crate::{
	errors::CatBridgeError,
	fsemul::pcfs::sata::proto::{SataPacketHeader, construct_sata_response},
};

/// A packet to change the owner of a file.
///
/// This will always, literally always fail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataChangeOwnerPacketBody {
	/// The path to query, note that this is not the 'resolved' path which is
	/// the path to actual read from.
	///
	/// Interpolation has a few known ways of being replaced:
	///
	/// - `%MLC_EMU_DIR`: `<cafe_sdk>/data/mlc/`
	/// - `%SLC_EMU_DIR`: `<cafe_sdk>/data/slc/`
	/// - `%DISC_EMU_DIR`: `<cafe_sdk>/data/disc/`
	/// - `%SAVE_EMU_DIR`: `<cafe_sdk>/data/save/`
	/// - `%NETWORK`: <mounted network share path>
	path: String,
	/// The UID to set the owner of this file as.
	uid: u32,
	/// The GID to set the owner of this file as.
	gid: u32,
}

impl SataChangeOwnerPacketBody {
	/// Attempt to construct a new change owner packet.
	///
	/// ## Errors
	///
	/// If the path is longer than 511 bytes. Normally the max path is 512 bytes,
	/// but because we need to encode our data as a C-String with a NUL
	/// terminator we cannot be longer than 511 bytes.
	///
	/// Consider using relative/mapped paths if possible when dealing with long
	/// paths.
	pub fn new(path: String, uid: u32, gid: u32) -> Result<Self, PCFSApiError> {
		if path.len() > 511 {
			return Err(PCFSApiError::PathTooLong(path));
		}

		Ok(Self { path, uid, gid })
	}

	#[must_use]
	pub fn path(&self) -> &str {
		self.path.as_str()
	}

	/// Update the path to send in this particular change owner packet.
	///
	/// ## Errors
	///
	/// If the path is longer than 511 bytes. Normally the max path is 512 bytes,
	/// but because we need to encode our data as a C-String with a NUL
	/// terminator we cannot be longer than 511 bytes.
	///
	/// Consider using relative/mapped paths if possible when dealing with long
	/// paths.
	pub fn set_path(&mut self, new_path: String) -> Result<(), PCFSApiError> {
		if new_path.len() > 511 {
			return Err(PCFSApiError::PathTooLong(new_path));
		}

		self.path = new_path;
		Ok(())
	}

	#[must_use]
	pub const fn uid(&self) -> u32 {
		self.uid
	}

	/// Set the user id to send for this packet.
	pub const fn set_uid(&mut self, new_uid: u32) {
		self.uid = new_uid;
	}

	#[must_use]
	pub const fn gid(&self) -> u32 {
		self.gid
	}

	/// Set the group id to send for this packet.
	pub const fn set_gid(&mut self, new_gid: u32) {
		self.gid = new_gid;
	}

	/// Handle a change owner request.
	///
	/// This will always fail.
	///
	/// ## Errors
	///
	/// If we cannot construct a sata response packet because our data to send
	/// was somehow too large (this should never happen).
	#[cfg(feature = "servers")]
	pub fn handle(&self, request_header: &SataPacketHeader) -> Result<Bytes, CatBridgeError> {
		let mut buff = BytesMut::with_capacity(4);
		// We always error.
		buff.put_u32(0xFFF0_FFE0);

		Ok(construct_sata_response(request_header, 0, buff.freeze())?)
	}
}

impl TryFrom<Bytes> for SataChangeOwnerPacketBody {
	type Error = NetworkParseError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x208 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataChangeMode",
				"Body",
				0x208,
				value.len(),
				value,
			));
		}
		if value.len() > 0x208 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataChangeMode",
				value.slice(0x208..),
			));
		}

		let (path_bytes, num) = value.split_at(0x200);
		let path_c_str =
			CStr::from_bytes_until_nul(path_bytes).map_err(NetworkParseError::BadCString)?;
		let uid = u32::from_be_bytes([num[0], num[1], num[2], num[3]]);
		let gid = u32::from_be_bytes([num[4], num[5], num[6], num[7]]);
		let final_path = path_c_str.to_str()?.to_owned();

		Ok(Self {
			path: final_path,
			uid,
			gid,
		})
	}
}

impl From<&SataChangeOwnerPacketBody> for Bytes {
	fn from(value: &SataChangeOwnerPacketBody) -> Self {
		let mut result = BytesMut::with_capacity(0x208);
		result.extend_from_slice(value.path.as_bytes());
		// These are C Strings so we need a NUL terminator.
		// Pad with `0`, til we get a full path with a nul terminator.
		result.extend(BytesMut::zeroed(0x200 - result.len()));
		result.put_u32(value.uid);
		result.put_u32(value.gid);
		result.freeze()
	}
}

impl From<SataChangeOwnerPacketBody> for Bytes {
	fn from(value: SataChangeOwnerPacketBody) -> Self {
		Self::from(&value)
	}
}

const SATA_CHANGE_OWNER_PACKET_BODY_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("path"),
	NamedField::new("uid"),
	NamedField::new("gid"),
];

impl Structable for SataChangeOwnerPacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataChangeOwnerPacketBody",
			Fields::Named(SATA_CHANGE_OWNER_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataChangeOwnerPacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_CHANGE_OWNER_PACKET_BODY_FIELDS,
			&[
				Valuable::as_value(&self.path),
				Valuable::as_value(&self.uid),
				Valuable::as_value(&self.gid),
			],
		));
	}
}

#[cfg(test)]
mod unit_tests {
	#[cfg(feature = "servers")]
	use super::*;

	#[cfg(feature = "servers")]
	#[tokio::test]
	pub async fn change_mode_request() {
		let request = SataChangeOwnerPacketBody {
			path: "/%SLC_EMU_DIR/to-query/file.txt".to_owned(),
			uid: 0,
			gid: 0,
		};
		let mocked_header = SataPacketHeader {
			packet_data_len: 0,
			packet_id: 0,
			flags: 0,
			version: 0,
			timestamp_on_host: 0,
			pid_on_host: 0,
		};

		let mut response = request
			.handle(&mocked_header)
			.expect("Failed to handle change owner!");
		assert_eq!(response.len(), 4 + 0x20, "Packet is not correct size!");
		// Okay first chop off the header, we don't care.
		_ = response.split_to(0x20);
		assert_eq!(
			response,
			Bytes::from(vec![
				0xFF, 0xF0, 0xFF, 0xE0, // RC
			]),
		);
	}
}
