//! Definitions, and handlers for the `GetQueryByInfo` packet type.
//!
//! Although this name implies there's some sort of querying, or something
//! going on there's actually not. You simply give us a file path, and we
//! give you either file information, the count of files in a directory, or
//! the size of a particular file. Wow.

use crate::{
	errors::NetworkParseError,
	fsemul::pcfs::errors::{PCFSApiError, SataProtocolError},
};
use bytes::{BufMut, Bytes, BytesMut};
use std::ffi::CStr;
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

#[cfg(feature = "clients")]
use bytes::Buf;

#[cfg(feature = "servers")]
use crate::{
	errors::{CatBridgeError, FSError},
	fsemul::{
		HostFilesystem,
		host_filesystem::{FilesystemLocation, ResolvedLocation},
		pcfs::sata::proto::{SataPacketHeader, construct_sata_response},
	},
};
#[cfg(feature = "servers")]
use std::{
	fs::read_dir,
	path::PathBuf,
	sync::LazyLock,
	time::{Duration, SystemTime},
};
#[cfg(feature = "servers")]
use sysinfo::{Disk, Disks};
#[cfg(feature = "servers")]
use tracing::{debug, warn};
#[cfg(feature = "servers")]
use walkdir::WalkDir;

#[cfg(feature = "servers")]
/// A folder is required to call this particular api.
const FOLDER_REQUIRED_ERROR: u32 = 0xFFF0_FFD7;
#[cfg(feature = "servers")]
/// Returned when a directory is too large to walk.
const SIZE_TOO_BIG_ERROR: u32 = 0xFFF0_FFE7;
#[cfg(feature = "servers")]
/// An error code to send when a path does not exist.
///
/// This is also used in some places that are a bit of a stretch like for
/// network shares on disk space. The path doesn't exist on a disk, so this
/// error code is used, even if it's not quite exact.
const PATH_NOT_EXIST_ERROR: u32 = 0xFFF0_FFE9;
#[cfg(feature = "servers")]
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

	/// Handle a Get Info Query request.
	///
	/// This handles getting the types of information from the filesystem.
	///
	/// ## Errors
	///
	/// If we cannot construct a sata response packet because our data to send
	/// was somehow too large (this should ideally never happen).
	#[cfg(feature = "servers")]
	pub async fn handle(
		&self,
		request_header: &SataPacketHeader,
		host_filesystem: &HostFilesystem,
	) -> Result<Bytes, CatBridgeError> {
		let Ok(final_location) = host_filesystem.resolve_path(&self.path) else {
			return Ok(Self::error_with_code(request_header, PATH_NOT_EXIST_ERROR)?);
		};

		match self.typ {
			PCFSSataQueryType::FreeDiskSpace => {
				Self::handle_disk_space(request_header, final_location)
			}
			PCFSSataQueryType::SizeOfFolder => {
				Self::handle_folder_size(request_header, final_location)
			}
			PCFSSataQueryType::FileCount => Self::handle_file_count(request_header, final_location),
			PCFSSataQueryType::FileDetails => {
				Self::handle_file_info(host_filesystem, request_header, final_location).await
			}
		}
	}

	/// Get the file information given a particular already open fd.
	///
	/// ## Errors
	///
	/// If we cannot construct a sata response packet.
	#[cfg(feature = "servers")]
	pub async fn stat_fd(
		request_header: &SataPacketHeader,
		host_filesystem: &HostFilesystem,
		fd: i32,
	) -> Result<Bytes, CatBridgeError> {
		let path = {
			let Some(entry) = host_filesystem.get_file(fd).await else {
				debug!(
					packet.fd = fd,
					packet.typ = "GetInfoByQueryPacketBody::stat_fd",
					"Processing stat of already open fd",
				);

				return Ok(Self::error_with_code(request_header, PATH_NOT_EXIST_ERROR)?);
			};
			entry.2.clone()
		};

		Self::handle_file_info(
			host_filesystem,
			request_header,
			ResolvedLocation::Filesystem(FilesystemLocation::new(path.clone(), path, true)),
		)
		.await
	}

	/// Get the information for a particular path.
	///
	/// ## Errors
	///
	/// If the path metadata can not be retrieved.
	#[cfg(feature = "servers")]
	pub async fn info_for_path(
		host_filesystem: &HostFilesystem,
		path: &PathBuf,
	) -> Result<Bytes, FSError> {
		let path_metadata = path.metadata()?;

		let mut response = BytesMut::with_capacity(84);
		response.put_u32(if path_metadata.is_file() {
			0x2C00_0000
		} else {
			0xAC00_0000
		});

		let is_read_only = if path_metadata.is_dir() {
			host_filesystem.directory_is_read_only(path).await
		} else {
			path_metadata.permissions().readonly()
		};

		// Because this was built for windows, it must be one of the two.
		response.put_u32(if is_read_only { 0x444 } else { 0x666 });
		// These are always hardcoded to 1.
		response.put_u32(1);
		response.put_u32(1);
		response.put_u32(if path_metadata.is_dir() {
			0
		} else {
			u32::try_from(path_metadata.len()).unwrap_or(u32::MAX)
		});
		// Constants.
		response.put_u32(0);
		response.put_u32(0xE8);
		response.put_u32(0xDA6F_F000);
		response.put_u32(0);
		response.put_u64(
			u64::try_from(
				path_metadata
					.created()
					.unwrap_or(SystemTime::now())
					.duration_since(*FAT_TIMESTAMP_START)
					.unwrap_or(Duration::from_secs(0))
					.as_millis(),
			)
			.unwrap_or(u64::MAX),
		);
		response.put_u64(
			u64::try_from(
				path_metadata
					.created()
					.unwrap_or(SystemTime::now())
					.duration_since(*FAT_TIMESTAMP_START)
					.unwrap_or(Duration::from_secs(0))
					.as_millis(),
			)
			.unwrap_or(u64::MAX),
		);
		response.extend_from_slice(&[0; 32]);
		Ok(response.freeze())
	}

	/// Get the total amount of free disk space a path would be in.
	#[cfg(feature = "servers")]
	fn handle_disk_space(
		request_header: &SataPacketHeader,
		location: ResolvedLocation,
	) -> Result<Bytes, CatBridgeError> {
		// The file needs to know which disk we're on.
		//
		// So the path needs to exist, or one of it's parent paths do, and it
		// needs to not be on the network....
		let ResolvedLocation::Filesystem(fs_location) = location else {
			debug!(
				packet.typ = "PCFSSrvGetInfo",
				packet.sub_type = "handle_disk_space",
				"Failed to resolve path!",
			);
			// Network locations cannot have a disk space to measure.
			return Ok(Self::error_with_code(request_header, PATH_NOT_EXIST_ERROR)?);
		};

		// Okay we have a path that exists, and is canonicalized.
		//
		// Let's take a look and identify what disk it's on, then get that
		// disks free space.
		let disks = Disks::new_with_refreshed_list();
		let mut disk_holding_path: Option<&Disk> = None;
		// Check for which disk this path will be stored on.
		//
		// E.g. find the mountpoint that is _most specific_.
		for potential_disk in &disks {
			let mount_point = potential_disk.mount_point();
			if fs_location.closest_resolved_path().starts_with(
				mount_point
					.canonicalize()
					.unwrap_or_else(|_| mount_point.to_path_buf()),
			) {
				let mut should_insert = true;
				if let Some(other_potential_source) = disk_holding_path {
					if other_potential_source.mount_point().components().count()
						> potential_disk.mount_point().components().count()
					{
						should_insert = false;
					}
				}
				if should_insert {
					_ = disk_holding_path.insert(potential_disk);
				}
			}
		}
		let Some(disk) = disk_holding_path else {
			debug!(
				packet.typ = "PCFSSrvGetInfo",
				packet.sub_type = "handle_disk_space",
				"Failed to find root disk!",
			);

			// This location must be some sort of network mount, ignore.
			return Ok(Self::error_with_code(request_header, PATH_NOT_EXIST_ERROR)?);
		};

		let mut response = BytesMut::with_capacity(88);
		response.put_u32(0);
		response.put_u64(disk.available_space());
		response.extend_from_slice(&[0; 76]);
		Ok(construct_sata_response(request_header, 0, response)?)
	}

	/// Get the total size of a folder, must be able to fit within a [`u32`].
	#[cfg(feature = "servers")]
	fn handle_folder_size(
		request_header: &SataPacketHeader,
		location: ResolvedLocation,
	) -> Result<Bytes, CatBridgeError> {
		let ResolvedLocation::Filesystem(fs_location) = location else {
			todo!("network shares not yet implemented!")
		};
		// This means our path doesn't exist, so we can't iterate over our folder
		// because it doesn't exist.
		if !fs_location.canonicalized_is_exact() {
			debug!(
				packet.typ = "PCFSSrvGetInfo",
				packet.sub_type = "handle_folder_size",
				"Failed to resolve path!",
			);
			return Ok(Self::error_with_code(request_header, PATH_NOT_EXIST_ERROR)?);
		}

		let mut total_size = 0_u64;
		// Okay we've got a folder time to iterate over it.
		for result in WalkDir::new(fs_location.closest_resolved_path())
			.follow_links(true)
			.follow_root_links(true)
		{
			let dir_entry = match result {
				Ok(p) => p,
				Err(cause) => {
					warn!(
						?cause,
						"Failed to iterate over directory, skipping will not be included in file size.",
					);
					continue;
				}
			};
			let metadata = match dir_entry.metadata() {
				Ok(md) => md,
				Err(cause) => {
					warn!(
					  ?cause,
					  path = %dir_entry.path().display(),
					  "Failed to get metadata for file, skipping will not be included in file size.",
					);
					continue;
				}
			};
			if !metadata.is_file() {
				continue;
			}

			total_size = total_size.saturating_add(metadata.len());
		}

		if total_size > u64::from(u32::MAX) {
			warn!(
				packet.typ = "PCFSSrvGetInfo",
				packet.sub_type = "handle_folder_size",
				"Folder size is too large, cannot fit in u32 this may result in errors on a real cat-dev!",
			);
		}
		let mut response = BytesMut::with_capacity(88);
		response.put_u32(0);
		response.put_u64(total_size);
		response.extend_from_slice(&[0; 76]);
		Ok(construct_sata_response(request_header, 0, response)?)
	}

	/// Get the total amount of files in a directory _non-recursively_.
	#[cfg(feature = "servers")]
	fn handle_file_count(
		request_header: &SataPacketHeader,
		location: ResolvedLocation,
	) -> Result<Bytes, CatBridgeError> {
		let ResolvedLocation::Filesystem(fs_location) = location else {
			todo!("network shares not yet implemented!")
		};
		// This means our path doesn't exist, so we can't iterate over our folder
		// because it doesn't exist.
		if !fs_location.canonicalized_is_exact() {
			debug!(
				packet.typ = "PCFSSrvGetInfo",
				packet.sub_type = "handle_file_count",
				"Failed to resolve path!",
			);
			return Ok(Self::error_with_code(request_header, PATH_NOT_EXIST_ERROR)?);
		}

		if !fs_location.resolved_path().is_dir() {
			debug!(
				packet.typ = "PCFSSrvGetInfo",
				packet.sub_type = "handle_folder_size",
				"Resolved location was not a directory!",
			);

			return Ok(Self::error_with_code(
				request_header,
				FOLDER_REQUIRED_ERROR,
			)?);
		}

		let Ok(iterator) = read_dir(fs_location.resolved_path()) else {
			debug!(
				packet.typ = "PCFSSrvGetInfo",
				packet.sub_type = "handle_folder_size",
				"Failed to open up iterator over directory!",
			);

			return Ok(Self::error_with_code(request_header, PATH_NOT_EXIST_ERROR)?);
		};

		let mut count = 0_u32;
		for result in iterator {
			if let Err(cause) = result {
				warn!(
					?cause,
					"Failed to iterate over directory, skipping will not be included in file count.",
				);
				continue;
			}

			if count == u32::MAX {
				warn!(
					cause = "too_many_files",
					"Failed to iterate over directory, file contains more than u32::MAX!",
				);
				return Ok(Self::error_with_code(request_header, SIZE_TOO_BIG_ERROR)?);
			}
			count += 1;
		}

		let mut response = BytesMut::with_capacity(88);
		response.put_u32(0);
		response.put_u32(count);
		response.extend_from_slice(&[0; 80]);

		Ok(construct_sata_response(
			request_header,
			0,
			response.freeze(),
		)?)
	}

	/// Get information about a particular file on disk.
	#[allow(
		// Will be needed for network shares.
		clippy::unused_async,
	)]
	#[cfg(feature = "servers")]
	async fn handle_file_info(
		fs: &HostFilesystem,
		request_header: &SataPacketHeader,
		location: ResolvedLocation,
	) -> Result<Bytes, CatBridgeError> {
		match location {
			ResolvedLocation::Filesystem(ref filesystem) => {
				let Ok(info) = Self::info_for_path(fs, filesystem.resolved_path()).await else {
					debug!(
						packet.typ = "PCFSSrvGetInfo",
						packet.sub_type = "handle_file_info",
						"Failed to resolve path!",
					);

					return Ok(Self::error_with_code(request_header, PATH_NOT_EXIST_ERROR)?);
				};

				let mut response = BytesMut::with_capacity(88);
				response.put_u32(0);
				response.extend(info);

				debug!(
					packet.typ = "PCFSSrvGetInfo",
					packet.sub_type = "handle_file_info",
					"Successfully stat'd file!",
				);
				Ok(construct_sata_response(
					request_header,
					0,
					response.freeze(),
				)?)
			}
			ResolvedLocation::Network(ref _network) => {
				todo!("Network shares not yet implemented!");
			}
		}
	}

	#[cfg(feature = "servers")]
	fn error_with_code(
		request_header: &SataPacketHeader,
		error: u32,
	) -> Result<Bytes, PCFSApiError> {
		let mut buff = BytesMut::with_capacity(88);
		buff.put_u32(error);
		buff.extend_from_slice(&[0; 84]);

		construct_sata_response(request_header, 0, buff.freeze())
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

#[cfg(feature = "clients")]
/// A response that came from a particular query response.
///
/// These depends on the query type that was actively passed in. Most paths
/// just return a very basic "size" (e.g. file count, or file length, etc.).
/// However the file stat query type actually returns all the information about
/// a particular path.
pub enum PCFSSataQueryResponse {
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

#[cfg(feature = "clients")]
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

#[cfg(feature = "clients")]
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

#[cfg(feature = "clients")]
impl PCFSSataFdInfo {
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

#[cfg(feature = "clients")]
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
#[cfg(feature = "clients")]
impl From<PCFSSataFdInfo> for Bytes {
	fn from(value: PCFSSataFdInfo) -> Self {
		Self::from(&value)
	}
}

#[cfg(feature = "clients")]
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
	#[cfg(feature = "servers")]
	use super::*;
	#[cfg(feature = "servers")]
	use crate::fsemul::host_filesystem::test_helpers::{
		create_temporary_host_filesystem, join_many,
	};

	#[cfg(feature = "servers")]
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

	#[cfg(feature = "servers")]
	#[tokio::test]
	pub async fn disk_size_query_type() {
		// We don't know what your actual disk size here is in tests.
		//
		// BUT, we can confirm it populates the right bytes.
		let (_tempdir, fs) = create_temporary_host_filesystem().await;
		let request = SataGetInfoByQueryPacketBody {
			path: "/%MLC_EMU_DIR/".to_owned(),
			typ: PCFSSataQueryType::FreeDiskSpace,
		};
		let mocked_header = SataPacketHeader {
			packet_data_len: 0,
			packet_id: 0,
			flags: 0,
			version: 0,
			timestamp_on_host: 0,
			pid_on_host: 0,
		};

		let mut space = request
			.handle(&mocked_header, &fs)
			.await
			.expect("Failed to handle free disk space request!");
		assert_eq!(space.len(), 88 + 0x20, "Packet is not correct size!");
		// Okay first chop off the header, we don't care.
		_ = space.split_to(0x20);
		assert_eq!(
			[space[0], space[1], space[2], space[3]],
			[0, 0, 0, 0],
			"RC of free disk space must be all 0's!"
		);
		assert_ne!(
			[
				space[4], space[5], space[6], space[7], space[8], space[9], space[10], space[11]
			],
			[0, 0, 0, 0, 0, 0, 0, 0],
		);
		assert_eq!(
			&space[12..],
			&[0; 76],
			"Trailer of free disk space must be empty!"
		);
	}

	#[cfg(feature = "servers")]
	#[tokio::test]
	pub async fn size_of_folder_query_type() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
		let request = SataGetInfoByQueryPacketBody {
			path: "/%MLC_EMU_DIR/my-directory/".to_owned(),
			typ: PCFSSataQueryType::SizeOfFolder,
		};
		let mocked_header = SataPacketHeader {
			packet_data_len: 0,
			packet_id: 0,
			flags: 0,
			version: 0,
			timestamp_on_host: 0,
			pid_on_host: 0,
		};

		// Okay let's create some files and some sizes in there.
		let base_dir = join_many(tempdir.path(), ["data", "mlc", "my-directory"]);
		tokio::fs::create_dir(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		tokio::fs::write(join_many(&base_dir, ["my-file.txt"]), vec![0; 8192])
			.await
			.expect("Failed to write test file!");
		// This should be recursive so we're going to check.
		let sub_dir = join_many(tempdir.path(), ["data", "mlc", "my-directory", "other"]);
		tokio::fs::create_dir(&sub_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		tokio::fs::write(join_many(&sub_dir, ["my-other-file.txt"]), vec![0; 4096])
			.await
			.expect("Failed to write other test file!");

		let mut response = request
			.handle(&mocked_header, &fs)
			.await
			.expect("Failed to handle size of folder query!");
		assert_eq!(response.len(), 88 + 0x20, "Packet is not correct size!");
		// Okay first chop off the header, we don't care.
		_ = response.split_to(0x20);
		// RC + padding bytes.
		assert_eq!(
			[
				response[0],
				response[1],
				response[2],
				response[3],
				response[4],
				response[5],
				response[6],
				response[7]
			],
			[0_u8; 8],
			"Header was not 8 empty bytes!",
		);
		// Folder size! In our case, 4096 + 8192
		assert_eq!(
			[response[8], response[9], response[10], response[11]],
			[0x00, 0x00, 0x30, 0x00],
			"Calculated folder size was not correct!",
		);
		assert_eq!(
			&response[12..],
			&[0; 76],
			"Trailer of folder size must be empty!"
		);
	}

	#[cfg(feature = "servers")]
	#[tokio::test]
	pub async fn file_count_query_type() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
		let request = SataGetInfoByQueryPacketBody {
			path: "/%SLC_EMU_DIR/my-directory/".to_owned(),
			typ: PCFSSataQueryType::FileCount,
		};
		let mocked_header = SataPacketHeader {
			packet_data_len: 0,
			packet_id: 0,
			flags: 0,
			version: 0,
			timestamp_on_host: 0,
			pid_on_host: 0,
		};

		// Okay let's create some files and some sizes in there.
		let base_dir = join_many(tempdir.path(), ["data", "slc", "my-directory"]);
		tokio::fs::create_dir(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		tokio::fs::write(join_many(&base_dir, ["my-file.txt"]), vec![0; 8192])
			.await
			.expect("Failed to write test file!");
		// This shouldn't be recursive so we're going to check.
		//
		// It will count the directory though.
		let sub_dir = join_many(tempdir.path(), ["data", "slc", "my-directory", "other"]);
		tokio::fs::create_dir(&sub_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		tokio::fs::write(join_many(&sub_dir, ["my-other-file.txt"]), vec![0; 4096])
			.await
			.expect("Failed to write other test file!");

		let mut response = request
			.handle(&mocked_header, &fs)
			.await
			.expect("Failed to handle count of folder query!");
		assert_eq!(response.len(), 88 + 0x20, "Packet is not correct size!");
		// Okay first chop off the header, we don't care.
		_ = response.split_to(0x20);
		// RC + padding bytes.
		assert_eq!(
			[response[0], response[1], response[2], response[3]],
			[0_u8; 4],
			"Header was not 4 empty bytes!",
		);
		// File count
		assert_eq!(
			[response[4], response[5], response[6], response[7]],
			[0x00, 0x00, 0x0, 0x02],
			"Calculated folder size was not correct!",
		);
		assert_eq!(
			&response[8..],
			&[0; 80],
			"Trailer of folder size must be empty!"
		);
	}

	#[cfg(feature = "servers")]
	#[tokio::test]
	pub async fn file_info_query_type() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
		let request = SataGetInfoByQueryPacketBody {
			path: "/%SLC_EMU_DIR/to-query/file.txt".to_owned(),
			typ: PCFSSataQueryType::FileDetails,
		};
		let mocked_header = SataPacketHeader {
			packet_data_len: 0,
			packet_id: 0,
			flags: 0,
			version: 0,
			timestamp_on_host: 0,
			pid_on_host: 0,
		};

		// Okay let's create some files and some sizes in there.
		let base_dir = join_many(tempdir.path(), ["data", "slc", "to-query"]);
		tokio::fs::create_dir(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		tokio::fs::write(join_many(&base_dir, ["file.txt"]), vec![0; 1307])
			.await
			.expect("Failed to write test file!");

		let mut response = request
			.handle(&mocked_header, &fs)
			.await
			.expect("Failed to handle file info query!");
		assert_eq!(response.len(), 88 + 0x20, "Packet is not correct size!");
		// Okay first chop off the header, we don't care.
		_ = response.split_to(0x20);
		assert_eq!(
			&response[..40],
			&[
				0x00, 0x00, 0x00, 0x00, // rc
				0x2c, 0x00, 0x00, 0x00, // flags (file == 0x2C000000, folder == 0xAC00000000)
				0x00, 0x00, 0x06,
				0x66, // Folder permissions (444 == read only, 666 == read & write)
				0x00, 0x00, 0x00, 0x01, // constant
				0x00, 0x00, 0x00, 0x01, // constant
				0x00, 0x00, 0x05, 0x1B, // File size
				0x00, 0x00, 0x00, 0x00, // constant
				0x00, 0x00, 0x00, 0xE8, // constant
				0xDA, 0x6F, 0xF0, 0x00, // constant
				0x00, 0x00, 0x00, 0x00, // constant
			],
		);
		// Timestamps!
		assert_ne!(
			[
				response[40],
				response[41],
				response[42],
				response[43],
				response[44],
				response[45],
				response[46],
				response[47]
			],
			[0_u8; 8],
		);
		assert_ne!(
			[
				response[48],
				response[49],
				response[50],
				response[51],
				response[52],
				response[53],
				response[54],
				response[55]
			],
			[0_u8; 8],
		);
		assert_eq!(&response[56..], &[0; 32]);
	}
}
