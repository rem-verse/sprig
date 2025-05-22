//! Definitions for the `GetQueryByInfo` packet type, and it's response types.
//!
//! Although this name implies there's some sort of querying, or something
//! going on there's actually not. You simply give us a file path, and we
//! give you either file information, the count of files in a directory, or
//! the size of a particular file. Wow.

use crate::{
	errors::NetworkParseError,
	fsemul::{
		HostFilesystem,
		pcfs::errors::{PCFSApiError, SataProtocolError},
	},
};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::{
	ffi::CStr,
	fs::Metadata,
	path::PathBuf,
	sync::LazyLock,
	time::{Duration, SystemTime},
};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

/// Timestamps are "FAT" timestamps which start in 1980.
static FAT_TIMESTAMP_START: LazyLock<SystemTime> = LazyLock::new(|| {
	SystemTime::UNIX_EPOCH
		.checked_add(Duration::from_secs(315_540_000))
		.expect("Failed to get timestamp for 1980! required!")
});

/// A packet to get information about a particular directory path.
///
/// This can do everything from "get the free space of the disk this path
/// is on", to "get my some metadata about this very specific path."
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SataGetInfoByQueryPacketBody {
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
	/// The type of information we're looking for.
	typ: PCFSSataQueryType,
}

impl SataGetInfoByQueryPacketBody {
	/// Attempt to construct a new packet to get info about some path.
	///
	/// ## Errors
	///
	/// If the path is longer than 511 bytes. Normally the max path is 512 bytes,
	/// but because we need to encode our data as a C-String with a NUL
	/// terminator we cannot be longer than 511 bytes.
	///
	/// Consider using relative/mapped paths if possible when dealing with long
	/// paths.
	pub fn new(path: String, query_type: PCFSSataQueryType) -> Result<Self, PCFSApiError> {
		if path.len() > 511 {
			return Err(PCFSApiError::PathTooLong(path));
		}

		Ok(Self {
			path,
			typ: query_type,
		})
	}

	#[must_use]
	pub const fn query_type(&self) -> PCFSSataQueryType {
		self.typ
	}

	pub const fn set_query_type(&mut self, new_type: PCFSSataQueryType) {
		self.typ = new_type;
	}

	#[must_use]
	pub fn path(&self) -> &str {
		self.path.as_str()
	}

	/// Update the path to send in this particular get info by query packet.
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
}

impl From<&SataGetInfoByQueryPacketBody> for Bytes {
	fn from(value: &SataGetInfoByQueryPacketBody) -> Self {
		let mut result = BytesMut::with_capacity(0x204);
		result.extend_from_slice(value.path.as_bytes());
		// These are C Strings so we need a NUL terminator.
		// Pad with `0`, til we get a full path with a nul terminator.
		result.extend(BytesMut::zeroed(0x200 - result.len()));
		result.put_u32(u32::from(value.typ));
		result.freeze()
	}
}

impl From<SataGetInfoByQueryPacketBody> for Bytes {
	fn from(value: SataGetInfoByQueryPacketBody) -> Self {
		Self::from(&value)
	}
}

impl TryFrom<Bytes> for SataGetInfoByQueryPacketBody {
	type Error = NetworkParseError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 0x204 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"SataGetInfoByQuery",
				"Body",
				0x204,
				value.len(),
				value,
			));
		}
		if value.len() > 0x204 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"SataGetInfoByQueryBody",
				value.slice(0x204..),
			));
		}

		let (path_bytes, num) = value.split_at(0x200);
		let path_c_str =
			CStr::from_bytes_until_nul(path_bytes).map_err(NetworkParseError::BadCString)?;
		let query_type = u32::from_be_bytes([num[0], num[1], num[2], num[3]]);
		let final_path = path_c_str.to_str()?.to_owned();

		Ok(Self {
			path: final_path,
			typ: PCFSSataQueryType::try_from(query_type)?,
		})
	}
}

const SATA_GET_INFO_BY_QUERY_PACKET_BODY_FIELDS: &[NamedField<'static>] =
	&[NamedField::new("path"), NamedField::new("type")];

impl Structable for SataGetInfoByQueryPacketBody {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"SataGetInfoByQueryPacketBody",
			Fields::Named(SATA_GET_INFO_BY_QUERY_PACKET_BODY_FIELDS),
		)
	}
}

impl Valuable for SataGetInfoByQueryPacketBody {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			SATA_GET_INFO_BY_QUERY_PACKET_BODY_FIELDS,
			&[
				Valuable::as_value(&self.path),
				Valuable::as_value(&self.typ),
			],
		));
	}
}

/// The type of information we're looking for from our request.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Valuable)]
pub enum PCFSSataQueryType {
	/// Get the amount of free disk space available to the calling application.
	FreeDiskSpace,
	/// Get the size of files in a directory, recursively.
	SizeOfFolder,
	/// Get the number of files in a directory, recursively.
	FileCount,
	/// Get the information around a particular file or folder.
	FileDetails,
}

impl From<PCFSSataQueryType> for u32 {
	fn from(value: PCFSSataQueryType) -> Self {
		match value {
			PCFSSataQueryType::FreeDiskSpace => 0,
			PCFSSataQueryType::SizeOfFolder => 1,
			PCFSSataQueryType::FileCount => 2,
			PCFSSataQueryType::FileDetails => 5,
		}
	}
}

impl TryFrom<u32> for PCFSSataQueryType {
	type Error = SataProtocolError;

	fn try_from(value: u32) -> Result<Self, Self::Error> {
		match value {
			0 => Ok(Self::FreeDiskSpace),
			1 => Ok(Self::SizeOfFolder),
			2 => Ok(Self::FileCount),
			5 => Ok(Self::FileDetails),
			val => Err(SataProtocolError::UnknownGetInfoQueryType(val)),
		}
	}
}

/// A response that came from a particular query response.
///
/// These depends on the query type that was actively passed in. Most paths
/// just return a very basic "size" (e.g. file count, or file length, etc.).
/// However the file stat query type actually returns all the information about
/// a particular path.
#[derive(Debug, Valuable)]
pub enum PCFSSataQueryResponse {
	/// An error has occured, and we are returning an error code.
	ErrorCode(u32),
	/// A size response that is guaranteed to fit within a u32.
	///
	/// Query types that return this:
	///
	/// - [`PCFSSataQueryType::FileCount`]
	SmallSize(u32),
	/// A size response that is guaranteed to fit within a u64.
	///
	/// Query types that return this:
	///
	/// - [`PCFSSataQueryType::SizeOfFolder`]
	/// - [`PCFSSataQueryType::FreeDiskSpace`]
	LargeSize(u64),
	/// All the info about a particular file path.
	///
	/// Query types that return this:
	///
	/// - [`PCFSSataQueryType::FileDetails`]
	FDInfo(PCFSSataFdInfo),
}

impl PCFSSataQueryResponse {
	/// Try to read a [`PCFSSataQueryResponse::SmallSize`] from a full response
	/// body.
	///
	/// ## Errors
	///
	/// If the response had an error code, or we could not get all of the [`u32`]
	/// value of the body.
	pub fn try_from_small(mut value: Bytes) -> Result<Self, NetworkParseError> {
		let rc = value.get_u32();
		if rc != 0 {
			return Err(NetworkParseError::ErrorCode(rc));
		}

		let smol = value.get_u32();

		Ok(Self::SmallSize(smol))
	}

	/// Try to read a [`PCFSSataQueryResponse::LargeSize`] from a full response
	/// body.
	///
	/// ## Errors
	///
	/// If the response had an error code, or we could not get all of the [`u64`]
	/// value of the body.
	pub fn try_from_large(mut value: Bytes) -> Result<Self, NetworkParseError> {
		let rc = value.get_u32();
		if rc != 0 {
			return Err(NetworkParseError::ErrorCode(rc));
		}

		let larg = value.get_u64();

		Ok(Self::LargeSize(larg))
	}

	/// Try to read a [`PCFSSataQueryResponse::FDInfo`] from a full response
	/// body.
	///
	/// ## Errors
	///
	/// If the response had an error code, or we could not get the file info from
	/// the file.
	pub fn try_from_fd_info(mut value: Bytes) -> Result<Self, NetworkParseError> {
		let rc = value.get_u32();
		if rc != 0 {
			return Err(NetworkParseError::ErrorCode(rc));
		}

		let fd_info = PCFSSataFdInfo::try_from(value)?;

		Ok(Self::FDInfo(fd_info))
	}
}

impl From<&PCFSSataQueryResponse> for Bytes {
	fn from(value: &PCFSSataQueryResponse) -> Self {
		match value {
			PCFSSataQueryResponse::FDInfo(fd_info) => {
				let mut buff = BytesMut::with_capacity(88);
				buff.put_u32(0);
				buff.extend(Bytes::from(fd_info));
				buff.freeze()
			}
			PCFSSataQueryResponse::LargeSize(lorg) => {
				let mut buff = BytesMut::with_capacity(88);
				buff.put_u32(0);
				buff.put_u64(*lorg);
				buff.extend([0; 76]);
				buff.freeze()
			}
			PCFSSataQueryResponse::SmallSize(smol) => {
				let mut buff = BytesMut::with_capacity(88);
				buff.put_u32(0);
				buff.put_u32(*smol);
				buff.extend([0; 80]);
				buff.freeze()
			}
			PCFSSataQueryResponse::ErrorCode(ec) => {
				let mut buff = BytesMut::with_capacity(88);
				buff.put_u32(*ec);
				buff.extend_from_slice(&[0; 84]);
				buff.freeze()
			}
		}
	}
}

impl From<PCFSSataQueryResponse> for Bytes {
	fn from(value: PCFSSataQueryResponse) -> Self {
		Self::from(&value)
	}
}

#[derive(Clone, Debug, PartialEq, Eq, Valuable)]
/// The info related to the file/directory of the path queried.
pub struct PCFSSataFdInfo {
	/// The raw underlying flags for the file/directory at the path queried.
	file_or_folder_flags: u32,
	/// The permissions bits for the file/directory at the path queried.
	perms: u32,
	/// The length of the file if this is an actual file, otherwise it _should_
	/// be set to 0.
	file_length: u32,
	/// A FAT-TS like timestamp for when this path was created.
	created_timestamp: u64,
	/// A FAT-TS like timestamp for when this path was last updated.
	last_updated_timestamp: u64,
}

impl PCFSSataFdInfo {
	/// File information for a particular file descriptor/path.
	#[must_use]
	pub async fn get_info(
		host_filesystem: &HostFilesystem,
		metadata: &Metadata,
		path: &PathBuf,
	) -> Self {
		let is_read_only = if metadata.is_dir() {
			host_filesystem.folder_is_read_only(path).await
		} else {
			metadata.permissions().readonly()
		};

		let file_or_folder_flags = if metadata.is_file() {
			0x2C00_0000
		} else {
			0xAC00_0000
		};
		let perms = if is_read_only { 0x444 } else { 0x666 };
		let file_length = if metadata.is_dir() {
			0
		} else {
			u32::try_from(metadata.len()).unwrap_or(u32::MAX)
		};
		let created_timestamp = u64::try_from(
			metadata
				.created()
				.unwrap_or(SystemTime::now())
				.duration_since(*FAT_TIMESTAMP_START)
				.unwrap_or(Duration::from_secs(0))
				.as_millis(),
		)
		.unwrap_or(u64::MAX);
		let updated_timestamp = u64::try_from(
			metadata
				.modified()
				.unwrap_or(SystemTime::now())
				.duration_since(*FAT_TIMESTAMP_START)
				.unwrap_or(Duration::from_secs(0))
				.as_millis(),
		)
		.unwrap_or(u64::MAX);

		Self {
			file_or_folder_flags,
			perms,
			file_length,
			created_timestamp,
			last_updated_timestamp: updated_timestamp,
		}
	}

	/// Get the raw file or folder type flags for a particular path.
	#[must_use]
	pub const fn flags(&self) -> u32 {
		self.file_or_folder_flags
	}

	/// Check if this path actually exists on disk.
	#[must_use]
	pub const fn exists(&self) -> bool {
		(self.file_or_folder_flags & 0x2000_0000) != 0
	}

	/// Check if this path was interpreted as a file.
	#[must_use]
	pub const fn is_file(&self) -> bool {
		(self.file_or_folder_flags & 0x8000_0000) == 0
	}

	/// Check if this path was interpreted as a directory.
	#[must_use]
	pub const fn is_directory(&self) -> bool {
		!self.is_file()
	}

	/// Get the unix permissions that exists on this file.
	///
	/// Given this is based originally on a windows filesystem, which really only
	/// has a natural equivalent for read only flags. You will either get
	/// `0x666`, or `0x444`.
	#[must_use]
	pub const fn permissions(&self) -> u32 {
		self.perms
	}

	/// The size of a file, if we are actually pointing at a file.
	#[must_use]
	pub const fn file_size(&self) -> Option<u32> {
		if self.is_file() {
			Some(self.file_length)
		} else {
			None
		}
	}

	/// A FAT-like timestamp that may be wrapped around.
	///
	/// Access the raw underlying value.
	#[must_use]
	pub const fn raw_created_timestamp(&self) -> u64 {
		self.created_timestamp
	}

	/// A FAT-like timestamp that may be wrapped around.
	///
	/// Access the raw underlying value.
	#[must_use]
	pub const fn raw_last_updated_timestamp(&self) -> u64 {
		self.last_updated_timestamp
	}
}

impl From<&PCFSSataFdInfo> for Bytes {
	fn from(value: &PCFSSataFdInfo) -> Self {
		let mut buff = BytesMut::with_capacity(84);
		buff.put_u32(value.file_or_folder_flags);
		buff.put_u32(value.perms);
		buff.put_u32(1);
		buff.put_u32(1);
		buff.put_u32(value.file_length);
		buff.put_u32(0);
		buff.put_u32(0xE8);
		buff.put_u32(0xDA6F_F000);
		buff.put_u32(0);
		buff.put_u64(value.created_timestamp);
		buff.put_u64(value.last_updated_timestamp);
		buff.extend_from_slice(&[0; 32]);
		buff.freeze()
	}
}

impl From<PCFSSataFdInfo> for Bytes {
	fn from(value: PCFSSataFdInfo) -> Self {
		Self::from(&value)
	}
}

impl TryFrom<Bytes> for PCFSSataFdInfo {
	type Error = NetworkParseError;

	fn try_from(mut value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 84 {
			return Err(NetworkParseError::FieldNotLongEnough(
				"PCFSSataFdInfo",
				"Body",
				84,
				value.len(),
				value,
			));
		}
		if value.len() > 84 {
			return Err(NetworkParseError::UnexpectedTrailer(
				"PCFSSataFdInfoBody",
				value.slice(84..),
			));
		}

		let fd_flags = value.get_u32();
		let unix_perms = value.get_u32();
		// skip two u32 that should always be 1
		_ = value.get_u32();
		_ = value.get_u32();
		let file_size = value.get_u32();
		// skip 4 u32's that should be various values
		_ = value.get_u32();
		_ = value.get_u32();
		_ = value.get_u32();
		_ = value.get_u32();
		// Get the timestamps.
		let created_ts = value.get_u64();
		let updated_ts = value.get_u64();
		// 32 0's

		Ok(Self {
			file_or_folder_flags: fd_flags,
			perms: unix_perms,
			file_length: file_size,
			created_timestamp: created_ts,
			last_updated_timestamp: updated_ts,
		})
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[test]
	pub fn query_types_to_and_fro() {
		for qt in vec![
			PCFSSataQueryType::FreeDiskSpace,
			PCFSSataQueryType::SizeOfFolder,
			PCFSSataQueryType::FileCount,
			PCFSSataQueryType::FileDetails,
		] {
			assert_eq!(Ok(qt), PCFSSataQueryType::try_from(u32::from(qt)));
		}
	}
}
