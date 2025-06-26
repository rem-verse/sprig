//! The WAL Mapper is responsible for turning a series of bytes into a
//! well-known packet type.

use crate::{
	errors::NetworkParseError,
	fsemul::pcfs::sata::{
		proto::{
			DirectoryItemResponse, MoveToFileLocation, SataCapabilitiesFlags,
			SataChangeModePacketBody, SataChangeOwnerPacketBody, SataCloseFilePacketBody,
			SataCloseFolderPacketBody, SataCreateFolderPacketBody, SataFDInfo,
			SataFileDescriptorResult, SataGetInfoByQueryPacketBody, SataOpenFilePacketBody,
			SataOpenFolderPacketBody, SataPingPacketBody, SataPongBody, SataQueryResponse,
			SataQueryType, SataReadFilePacketBody, SataReadFolderPacketBody, SataRemovePacketBody,
			SataRenamePacketBody, SataRequest, SataResponse, SataResultCode,
			SataRewindFolderPacketBody, SataSetFilePositionPacketBody, SataStatFilePacketBody,
			SataWriteFilePacketBody,
		},
		server::connection_flags::SataConnectionFlags,
	},
};
use bytes::Bytes;
use fnv::{FnvHashMap, FnvHasher};
use std::{
	fmt::{Display, Formatter, Result as FmtResult},
	hash::Hasher,
};

/// A request that is waiting for a response.
///
/// Used to keep track of which packets we're waiting on a response from incase
/// multiple queue up at once. This also holds the info a repsonse may need in
/// order to provide future logging.
///
/// E.g. in an open file request we send the path, but the response just sends
/// back the fd. In order to create a map of fd -> path (so we can log what's
/// happening to specific paths), we need info from the request when we process
/// the repsonse. So we can create that ideal mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WaitingRequest {
	/// A create folder request (Path, Will Set Write Mode).
	CreateFolder(String, bool),
	/// An open folder request (Path).
	OpenFolder(String),
	/// A read folder request (Path).
	ReadFolder(String),
	/// Rewind a folder iterator (Path).
	RewindFolder(String),
	/// Close a folder (Path).
	CloseFolder(i32, String),
	/// A request to open a file (Path, Mode)
	OpenFile(String, String),
	/// A request to read a file (Path, Move to file location before reading, Allow truncation).
	ReadFile(String, Option<MoveToFileLocation>, u32),
	/// A request to write to a file (Path, Move to file location before writing).
	WriteFile(String, Option<MoveToFileLocation>),
	/// A request to move the location of a particular file.
	SetFilePosition(String, MoveToFileLocation),
	/// Get the information about an already open file descriptor (Path).
	StatFile(String),
	/// Close an already open file (Path).
	CloseFile(i32, String),
	/// Remove a file or folder from the host filesystem (Path).
	Remove(String),
	/// Rename a file or folder from the host filesystem (Path, Path).
	Rename(String, String),
	/// Query the information about a particular path on the host fs (Path).
	QueryPath(SataQueryType, String),
	/// Change ownership of a particular path on the host filesystem (Path, uid, gid).
	ChangeOwner(String, u32, u32),
	/// Change the mode of a particular path on the host filesystem (Path, unix mode string).
	ChangeMode(String, String),
	/// A ping request (FFIO enabled, CSR enabled, first read size, first write size).
	Ping(bool, bool, u32, u32),
	/// If the packet is just some unknown bytes...
	Unknown(Bytes),
}

impl WaitingRequest {
	/// Parse out a Sata Request that is going to be waiting for a response.
	#[allow(
    // Overall each branch is relatively small, we just have a lot of packet types.
    clippy::too_many_lines,
  )]
	#[must_use]
	pub fn parse(
		fd_map: &FnvHashMap<i32, String>,
		folder_map: &FnvHashMap<i32, String>,
		data: Bytes,
	) -> Self {
		let Ok(req) = SataRequest::<Bytes>::parse_opaque(data.clone()) else {
			return Self::Unknown(data);
		};
		let (header, ci, body) = req.into_parts();

		match ci.command() {
			0x00 => {
				if let Ok(body) = SataCreateFolderPacketBody::try_from(body) {
					Self::CreateFolder(body.path().to_owned(), body.will_set_write_mode())
				} else {
					Self::Unknown(data)
				}
			}
			0x01 => {
				if let Ok(body) = SataOpenFolderPacketBody::try_from(body) {
					Self::OpenFolder(body.path().to_owned())
				} else {
					Self::Unknown(data)
				}
			}
			0x02 => {
				if let Ok(read_folder) = SataReadFolderPacketBody::try_from(body) {
					if let Some(path) = folder_map.get(&read_folder.file_descriptor()) {
						Self::ReadFolder(path.clone())
					} else {
						Self::ReadFolder(format!("<Unknown; {}>", read_folder.file_descriptor()))
					}
				} else {
					Self::Unknown(data)
				}
			}
			0x03 => {
				if let Ok(rewind_folder) = SataRewindFolderPacketBody::try_from(body) {
					if let Some(path) = folder_map.get(&rewind_folder.file_descriptor()) {
						Self::RewindFolder(path.clone())
					} else {
						Self::RewindFolder(format!(
							"<Unknown; {}>",
							rewind_folder.file_descriptor()
						))
					}
				} else {
					Self::Unknown(data)
				}
			}
			0x04 => {
				if let Ok(close_folder) = SataCloseFolderPacketBody::try_from(body) {
					if let Some(path) = folder_map.get(&close_folder.file_descriptor()) {
						Self::CloseFolder(close_folder.file_descriptor(), path.clone())
					} else {
						Self::CloseFolder(
							close_folder.file_descriptor(),
							format!("<Unknown; {}>", close_folder.file_descriptor()),
						)
					}
				} else {
					Self::Unknown(data)
				}
			}
			0x05 => {
				if let Ok(open_file) = SataOpenFilePacketBody::try_from(body) {
					Self::OpenFile(open_file.path().to_owned(), open_file.mode().to_owned())
				} else {
					Self::Unknown(data)
				}
			}
			0x06 => {
				if let Ok(read_file) = SataReadFilePacketBody::try_from(body) {
					if let Some(path) = fd_map.get(&read_file.file_descriptor()) {
						Self::ReadFile(
							path.clone(),
							if read_file.should_move() {
								Some(read_file.move_to_pointer())
							} else {
								None
							},
							read_file.block_count() * read_file.block_size(),
						)
					} else {
						Self::ReadFile(
							format!("<Unknown {}>", read_file.file_descriptor()),
							if read_file.should_move() {
								Some(read_file.move_to_pointer())
							} else {
								None
							},
							read_file.block_count() * read_file.block_size(),
						)
					}
				} else {
					Self::Unknown(data)
				}
			}
			0x07 => {
				if let Ok(write_file) = SataWriteFilePacketBody::try_from(body) {
					if let Some(path) = fd_map.get(&write_file.file_descriptor()) {
						Self::WriteFile(
							path.clone(),
							if write_file.should_move() {
								Some(write_file.move_to_pointer())
							} else {
								None
							},
						)
					} else {
						Self::WriteFile(
							format!("<Unknown {}>", write_file.file_descriptor()),
							if write_file.should_move() {
								Some(write_file.move_to_pointer())
							} else {
								None
							},
						)
					}
				} else {
					Self::Unknown(data)
				}
			}
			0x09 => {
				if let Ok(set_file_pos) = SataSetFilePositionPacketBody::try_from(body) {
					if let Some(path) = fd_map.get(&set_file_pos.file_descriptor()) {
						Self::SetFilePosition(path.clone(), set_file_pos.move_to_pointer())
					} else {
						Self::SetFilePosition(
							format!("<Unknown; {}>", set_file_pos.file_descriptor()),
							set_file_pos.move_to_pointer(),
						)
					}
				} else {
					Self::Unknown(data)
				}
			}
			0x0B => {
				if let Ok(stat_file) = SataStatFilePacketBody::try_from(body) {
					if let Some(path) = fd_map.get(&stat_file.file_descriptor()) {
						Self::StatFile(path.clone())
					} else {
						Self::StatFile(format!("<Unknown; {}>", stat_file.file_descriptor()))
					}
				} else {
					Self::Unknown(data)
				}
			}
			0x0D => {
				if let Ok(close_file) = SataCloseFilePacketBody::try_from(body) {
					if let Some(path) = fd_map.get(&close_file.file_descriptor()) {
						Self::CloseFile(close_file.file_descriptor(), path.clone())
					} else {
						Self::CloseFile(
							close_file.file_descriptor(),
							format!("<Unknown; {}>", close_file.file_descriptor()),
						)
					}
				} else {
					Self::Unknown(data)
				}
			}
			0x0E => {
				if let Ok(remove) = SataRemovePacketBody::try_from(body) {
					Self::Remove(remove.path().to_owned())
				} else {
					Self::Unknown(data)
				}
			}
			0x0F => {
				if let Ok(ren) = SataRenamePacketBody::try_from(body) {
					Self::Rename(ren.source_path().to_owned(), ren.dest_path().to_owned())
				} else {
					Self::Unknown(data)
				}
			}
			0x10 => {
				if let Ok(query) = SataGetInfoByQueryPacketBody::try_from(body) {
					Self::QueryPath(query.query_type(), query.path().to_owned())
				} else {
					Self::Unknown(data)
				}
			}
			0x12 => {
				if let Ok(change_owner) = SataChangeOwnerPacketBody::try_from(body) {
					Self::ChangeOwner(
						change_owner.path().to_owned(),
						change_owner.uid(),
						change_owner.gid(),
					)
				} else {
					Self::Unknown(data)
				}
			}
			0x13 => {
				if let Ok(chmod) = SataChangeModePacketBody::try_from(body) {
					Self::ChangeMode(
						chmod.path().to_owned(),
						if chmod.will_set_write_mode() {
							"0666".to_owned()
						} else {
							"0444".to_owned()
						},
					)
				} else {
					Self::Unknown(data)
				}
			}
			0x14 => {
				if SataPingPacketBody::try_from(body).is_ok() {
					let flags = SataCapabilitiesFlags(header.flags());
					Self::Ping(
						flags.intersects(SataCapabilitiesFlags::FAST_FILE_IO_SUPPORTED),
						flags.intersects(SataCapabilitiesFlags::COMBINED_SEND_RECV_SUPPORTED),
						ci.user().0,
						ci.user().1,
					)
				} else {
					Self::Unknown(data)
				}
			}
			_ => Self::Unknown(data),
		}
	}
}

impl Display for WaitingRequest {
	#[allow(
    // Overall each branch is relatively small, we just have a lot of packet types.
    clippy::too_many_lines,
  )]
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		match self {
			Self::CreateFolder(path, will_set_write_mode) => {
				write!(
					fmt,
					"CreateFolder {{path={path},mode={}}}",
					if *will_set_write_mode { "0666" } else { "0444" },
				)
			}
			Self::OpenFolder(path) => {
				write!(fmt, "OpenFolder   {{path={path}}}")
			}
			Self::ReadFolder(path) => {
				write!(fmt, "ReadFolder   {{path={path}}}")
			}
			Self::RewindFolder(path) => {
				write!(fmt, "RewindFolder {{path={path}}}")
			}
			Self::CloseFolder(_, path) => {
				write!(fmt, "CloseFolder  {{path={path}}}")
			}
			Self::OpenFile(path, mode) => {
				write!(fmt, "OpenFile     {{path={path},mode={mode}}}")
			}
			Self::ReadFile(path, move_to, read_size) => {
				write!(
					fmt,
					"ReadFile     {{path={path},move_to={},total_size={read_size}}}",
					if let Some(to) = move_to {
						format!("{to}")
					} else {
						"Nowhere".to_owned()
					}
				)
			}
			Self::WriteFile(path, move_to) => {
				write!(
					fmt,
					"WriteFile    {{path={path},move_to={}}}",
					if let Some(to) = move_to {
						format!("{to}")
					} else {
						"Nowhere".to_owned()
					}
				)
			}
			Self::SetFilePosition(path, move_to) => {
				write!(fmt, "SetFilePos   {{path={path},move_to={move_to}}}")
			}
			Self::StatFile(path) => {
				write!(fmt, "StatFile     {{path={path}}}")
			}
			Self::CloseFile(_, path) => {
				write!(fmt, "CloseFile    {{path={path}}}")
			}
			Self::Remove(path) => {
				write!(fmt, "Remove       {{path={path}}}")
			}
			Self::Rename(source, dest) => {
				write!(fmt, "Rename       {{source={source},dest={dest}}}")
			}
			Self::QueryPath(query_type, path) => {
				write!(
					fmt,
					"QueryPath    {{path={path},query_type={}}}",
					match query_type {
						SataQueryType::FileCount => "Files in Folder",
						SataQueryType::FileDetails => "Path Details",
						SataQueryType::FreeDiskSpace => "Free Space left on Disk",
						SataQueryType::SizeOfFolder => "Size of Folder",
					}
				)
			}
			Self::ChangeOwner(path, uid, gid) => {
				write!(fmt, "ChangeOwner  {{path={path},uid={uid},gid={gid}}}")
			}
			Self::ChangeMode(path, mode) => {
				write!(fmt, "ChangeMode   {{path={path},mode={mode}}}")
			}
			Self::Ping(ffio, csr, read_size, write_size) => {
				write!(
					fmt,
					"Ping         {{supports_ffio={ffio},supports_csr={csr},first_read_size={read_size},first_write_size={write_size}}}",
				)
			}
			Self::Unknown(data) => {
				write!(fmt, "Unknown      {{data={data:02x?}}}")
			}
		}
	}
}

#[derive(Debug, Clone)]
pub enum WaitingResponse {
	/// Create a new folder (rc).
	CreateFolder(SataResultCode),
	/// Open a folder (fd or rc).
	OpenFolder(SataFileDescriptorResult),
	/// Read the next file in a folder (RC, <Next File Name If Present>).
	ReadFolder(SataResultCode, Option<String>),
	/// Rewind the iterator for a folder (rc).
	RewindFolder(SataResultCode),
	/// Close an open folder (rc).
	CloseFolder(SataResultCode),
	/// Open an existing file (fd or rc).
	OpenFile(SataFileDescriptorResult),
	/// We read an open file <(file size as u32, bytes read, checksum), error code>.
	ReadFile(Result<(u32, usize, Bytes, u64), SataResultCode>),
	/// We wrote to a file (rc).
	WriteFile(SataResultCode),
	/// We moved the position in the file!
	SetFilePosition(SataResultCode),
	/// Queried info about an existing file (fd info).
	StatFile(Result<SataFDInfo, SataResultCode>),
	/// If we were able to close a file (rc).
	CloseFile(SataResultCode),
	/// If we were able to remove a file (rc).
	Remove(SataResultCode),
	/// If we were able to rename a file (rc).
	Rename(SataResultCode),
	/// The query results for a query (query response).
	QueryPath(Result<SataQueryResponse, SataResultCode>),
	/// If we were able to change ownership of a file (rc).
	ChangeOwner(SataResultCode),
	/// If we were able to change the mode of a file (rc).
	ChangeMode(SataResultCode),
	/// Response to a ping (ffio supported, csr supported).
	Pong(bool, bool),
	/// ??? -- Data.
	Unknown(Bytes),
}

impl WaitingResponse {
	/// Parse out the response into a printable thing.
	#[allow(
    // Overall each branch is relatively small, we just have a lot of packet types.
    clippy::too_many_lines,
  )]
	#[must_use]
	pub fn parse(flags: &SataConnectionFlags, request: &WaitingRequest, response: Bytes) -> Self {
		match request {
			WaitingRequest::CreateFolder(_, _) => {
				if let Ok(resp) = SataResponse::<SataResultCode>::try_from(response.clone()) {
					Self::CreateFolder(*resp.body())
				} else {
					Self::Unknown(response)
				}
			}
			WaitingRequest::OpenFolder(_) => {
				if let Ok(resp) =
					SataResponse::<SataFileDescriptorResult>::try_from(response.clone())
				{
					Self::OpenFolder(*resp.body())
				} else {
					Self::Unknown(response)
				}
			}
			WaitingRequest::ReadFolder(_) => {
				if let Ok(resp) = SataResponse::<DirectoryItemResponse>::try_from(response.clone())
				{
					Self::ReadFolder(
						SataResultCode(resp.body().return_code()),
						resp.body().clone().take_file_info().map(|item| item.1),
					)
				} else {
					Self::Unknown(response)
				}
			}
			WaitingRequest::RewindFolder(_) => {
				if let Ok(resp) = SataResponse::<SataResultCode>::try_from(response.clone()) {
					Self::RewindFolder(*resp.body())
				} else {
					Self::Unknown(response)
				}
			}
			WaitingRequest::CloseFolder(_, _) => {
				if let Ok(resp) = SataResponse::<SataResultCode>::try_from(response.clone()) {
					Self::CloseFolder(*resp.body())
				} else {
					Self::Unknown(response)
				}
			}
			WaitingRequest::OpenFile(_, _) => {
				if let Ok(resp) =
					SataResponse::<SataFileDescriptorResult>::try_from(response.clone())
				{
					Self::OpenFile(*resp.body())
				} else {
					Self::Unknown(response)
				}
			}
			WaitingRequest::ReadFile(_, _, _) => {
				if flags.ffio_enabled() {
					if response.len() < 0x24 {
						return Self::Unknown(response);
					}
					let file_len = u32::from_be_bytes([
						response[0x20],
						response[0x21],
						response[0x22],
						response[0x23],
					]);

					let mut chk = FnvHasher::with_key(69420);
					chk.write(&response);
					let header = response.slice(..0x20);
					Self::ReadFile(Ok((file_len, response.len(), header, chk.finish())))
				} else {
					todo!("Non-FFIO support")
				}
			}
			WaitingRequest::WriteFile(_, _) => {
				if let Ok(resp) = SataResponse::<SataResultCode>::try_from(response.clone()) {
					Self::WriteFile(*resp.body())
				} else {
					Self::Unknown(response)
				}
			}
			WaitingRequest::SetFilePosition(_, _) => {
				if let Ok(resp) = SataResponse::<SataResultCode>::try_from(response.clone()) {
					Self::SetFilePosition(*resp.body())
				} else {
					Self::Unknown(response)
				}
			}
			WaitingRequest::StatFile(_) => {
				if let Ok(resp) = SataResponse::<Bytes>::parse_opaque(response.clone()) {
					match SataQueryResponse::try_from_fd_info(resp.body().clone()) {
						Ok(qresp) => match qresp {
							SataQueryResponse::FDInfo(info) => Self::StatFile(Ok(info.clone())),
							SataQueryResponse::ErrorCode(ec) => {
								Self::StatFile(Err(SataResultCode(ec)))
							}
							_ => Self::Unknown(response),
						},
						Err(cause) => match cause {
							NetworkParseError::ErrorCode(rc) => {
								Self::StatFile(Err(SataResultCode(rc)))
							}
							_ => Self::Unknown(response),
						},
					}
				} else {
					Self::Unknown(response)
				}
			}
			WaitingRequest::CloseFile(_, _) => {
				if let Ok(resp) = SataResponse::<SataResultCode>::try_from(response.clone()) {
					Self::CloseFile(*resp.body())
				} else {
					Self::Unknown(response)
				}
			}
			WaitingRequest::Remove(_) => {
				if let Ok(resp) = SataResponse::<SataResultCode>::try_from(response.clone()) {
					Self::Remove(*resp.body())
				} else {
					Self::Unknown(response)
				}
			}
			WaitingRequest::Rename(_, _) => {
				if let Ok(resp) = SataResponse::<SataResultCode>::try_from(response.clone()) {
					Self::Rename(*resp.body())
				} else {
					Self::Unknown(response)
				}
			}
			WaitingRequest::QueryPath(query_type, _) => {
				if let Ok(resp) = SataResponse::<Bytes>::parse_opaque(response.clone()) {
					let inner_res = match query_type {
						SataQueryType::FileCount => {
							SataQueryResponse::try_from_small(resp.body().clone())
						}
						SataQueryType::FileDetails => {
							SataQueryResponse::try_from_fd_info(resp.body().clone())
						}
						SataQueryType::FreeDiskSpace | SataQueryType::SizeOfFolder => {
							SataQueryResponse::try_from_large(resp.body().clone())
						}
					};

					match inner_res {
						Ok(qi) => Self::QueryPath(Ok(qi)),
						Err(cause) => match cause {
							NetworkParseError::ErrorCode(ec) => {
								Self::QueryPath(Err(SataResultCode(ec)))
							}
							_ => Self::Unknown(response),
						},
					}
				} else {
					Self::Unknown(response)
				}
			}
			WaitingRequest::ChangeOwner(_, _, _) => {
				if let Ok(resp) = SataResponse::<SataResultCode>::try_from(response.clone()) {
					Self::ChangeOwner(*resp.body())
				} else {
					Self::Unknown(response)
				}
			}
			WaitingRequest::ChangeMode(_, _) => {
				if let Ok(resp) = SataResponse::<SataResultCode>::try_from(response.clone()) {
					Self::ChangeMode(*resp.body())
				} else {
					Self::Unknown(response)
				}
			}
			WaitingRequest::Ping(_, _, _, _) => {
				if let Ok(resp) = SataResponse::<SataPongBody>::try_from(response.clone()) {
					Self::Pong(
						resp.body().ffio_enabled(),
						resp.body().combined_send_recv_enabled(),
					)
				} else {
					Self::Unknown(response)
				}
			}
			WaitingRequest::Unknown(_) => Self::Unknown(response),
		}
	}
}

impl Display for WaitingResponse {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		match self {
			Self::CreateFolder(rc)
			| Self::ChangeOwner(rc)
			| Self::ChangeMode(rc)
			| Self::RewindFolder(rc)
			| Self::CloseFolder(rc)
			| Self::WriteFile(rc)
			| Self::CloseFile(rc)
			| Self::Remove(rc)
			| Self::Rename(rc)
			| Self::SetFilePosition(rc) => write!(fmt, "          RC {{return_code={rc}}}"),
			Self::OpenFolder(fdres) | Self::OpenFile(fdres) => {
				write!(fmt, "       FDRES {{response={fdres}}}")
			}
			Self::ReadFolder(rc, path) => write!(
				fmt,
				" Folder Item {{rc={rc},next_item={}}}",
				if let Some(item) = path {
					item.clone()
				} else {
					"<None>".to_owned()
				}
			),
			Self::ReadFile(result) => match result {
				Ok((file_size, read, header, chk)) => write!(
					fmt,
					"        READ {{chk={chk:02x},file_size={file_size},packet_len={read},header={header:02x?}}}",
				),
				Err(rc) => write!(fmt, "          RC {{return_code={rc}}}"),
			},
			Self::StatFile(result) => match result {
				Ok(fdi) => write!(
					fmt,
					"        STAT {{file_or_folder_flags={:02x},perms={:02x},file_length={}}}",
					fdi.flags(),
					fdi.permissions(),
					fdi.file_size().unwrap_or(0),
				),
				Err(rc) => write!(fmt, "          RC {{return_code={rc}}}"),
			},
			Self::QueryPath(result) => match result {
				Ok(qr) => write!(
					fmt,
					"       QUERY {{response={}}}",
					match qr {
						SataQueryResponse::ErrorCode(ec) => format!("ERROR:{ec}"),
						SataQueryResponse::FDInfo(info) => format!(
							"INFO:flags={:02x}:perms={:02x}:file_length={}",
							info.flags(),
							info.permissions(),
							info.file_size().unwrap_or(0),
						),
						SataQueryResponse::LargeSize(ls) => format!("LG:{ls}"),
						SataQueryResponse::SmallSize(ss) => format!("SM:{ss}"),
					}
				),
				Err(rc) => write!(fmt, "          RC {{return_code={rc}}}"),
			},
			Self::Pong(ffio, csr) => write!(
				fmt,
				"        PONG {{ffio_enabled={ffio},combined_send_recv_enabled={csr}}}",
			),
			Self::Unknown(data) => write!(fmt, "     Unknown {{data={data:02x?}}}"),
		}
	}
}
