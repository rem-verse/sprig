//! Client implementation for SATA over PCFS.

use crate::{
	errors::{CatBridgeError, NetworkError, NetworkParseError},
	fsemul::pcfs::{
		errors::SataProtocolError,
		sata::proto::{
			DirectoryItemResponse, MoveToFileLocation, SataCapabilitiesFlags,
			SataChangeModePacketBody, SataChangeOwnerPacketBody, SataCloseFilePacketBody,
			SataCloseFolderPacketBody, SataCommandInfo, SataCreateFolderPacketBody, SataFDInfo,
			SataFileDescriptorResult, SataGetInfoByQueryPacketBody, SataOpenFilePacketBody,
			SataPacketHeader, SataPingPacketBody, SataPongBody, SataQueryResponse, SataQueryType,
			SataReadFilePacketBody, SataReadFolderPacketBody, SataRemovePacketBody, SataRequest,
			SataResponse, SataResultCode, SataRewindFolderPacketBody,
			SataSetFilePositionPacketBody, SataStatFilePacketBody, SataWriteFilePacketBody,
		},
	},
	net::{
		client::TCPClient,
		models::{Endianness, NagleGuard},
	},
};
use bytes::{Buf, Bytes, BytesMut};
use std::{
	sync::{
		Arc,
		atomic::{AtomicBool, AtomicU32, Ordering},
	},
	time::Duration,
};
use tokio::net::ToSocketAddrs;
use valuable::Valuable;

/// Default PCFS-SATA Client timeout to use when we don't know where one is.
pub const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

/// A connection to a sata pcfs server.
#[derive(Debug, Valuable)]
pub struct SataClient {
	/// If we're actively supporting "CSR", or "Combined Send/Recv".
	supports_csr: Arc<AtomicBool>,
	/// If we're acctively supporting "FFIO", or "Fast File I/O".
	supports_ffio: Arc<AtomicBool>,
	/// Means something different to the official server, but for us means
	/// when padding stops being added.
	first_read_size: Arc<AtomicU32>,
	/// Means something different to the official server, but for us means
	/// when padding stops being added.
	first_write_size: Arc<AtomicU32>,
	/// The TCP Client we're warpping.
	underlying_client: TCPClient,
}

impl SataClient {
	/// Connect to a PCFS Sata server.
	///
	/// ## Errors
	///
	/// This errors when the client cannot connect to the server.
	pub async fn connect<AddrTy: ToSocketAddrs>(
		address: AddrTy,
		supports_csr: bool,
		supports_ffio: bool,
		first_read_size: u32,
		first_write_size: u32,
		trace_io_during_debug: bool,
	) -> Result<Self, CatBridgeError> {
		let client = TCPClient::new(
			"pcfs-sata",
			NagleGuard::U32LengthPrefixed(Endianness::Big, None),
			(None, None),
			trace_io_during_debug,
		);
		client.connect(address).await?;

		let this = Self {
			supports_csr: Arc::new(AtomicBool::new(supports_csr)),
			supports_ffio: Arc::new(AtomicBool::new(supports_ffio)),
			underlying_client: client,
			first_read_size: Arc::new(AtomicU32::new(first_read_size)),
			first_write_size: Arc::new(AtomicU32::new(first_write_size)),
		};
		this.ping(Some(DEFAULT_CLIENT_TIMEOUT)).await?;

		Ok(this)
	}

	/// Attempt to update Combined Send/Recv & Fast File I/O flags.
	///
	/// This may fail if setting to 'true', as both the server, and client have
	/// to support, and agree to these flags in order for them to be supported.
	///
	/// ## Errors
	///
	/// If we cannot negotiate with the server to update combined send/recv.
	pub async fn try_set_csr_ffio(&self, csr: bool, ffio: bool) -> Result<(), CatBridgeError> {
		self.supports_csr.store(csr, Ordering::Release);
		self.supports_ffio.store(ffio, Ordering::Release);
		self.ping(None).await?;
		Ok(())
	}

	/// Attempt to update combined send/recv flag or "CSR".
	///
	/// This may fail if setting to 'true', as both the server, and client have
	/// to support, and agree to support Combined Send/Recv.
	///
	/// ## Errors
	///
	/// If we cannot negotiate with the server to update combined send/recv.
	pub async fn try_set_csr(&self, csr: bool) -> Result<(), CatBridgeError> {
		self.supports_csr.store(csr, Ordering::Release);
		self.ping(None).await?;
		Ok(())
	}

	/// Attempt to update fast-file i/o or "FFIO".
	///
	/// This may fail if setting to 'true', as both the server, and client have
	/// to support, and agree to support FFIO.
	///
	/// ## Errors
	///
	/// If we cannot negotiate with the server to update combined send/recv.
	pub async fn try_set_ffio(&self, ffio: bool) -> Result<(), CatBridgeError> {
		self.supports_ffio.store(ffio, Ordering::Release);
		self.ping(None).await?;
		Ok(())
	}

	/// Perform a 'ping' on the remote host, and update our CSR/FFIO flag state.
	///
	/// ## Errors
	///
	/// If we cannot send a request to our upstream PCFS server, or if we cannot
	/// read a response back in the timeout specified.
	pub async fn ping(&self, timeout: Option<Duration>) -> Result<(), CatBridgeError> {
		let mut flags = SataCapabilitiesFlags::empty();
		if self.supports_csr.load(Ordering::Acquire) {
			flags = flags.union(SataCapabilitiesFlags::COMBINED_SEND_RECV_SUPPORTED);
		}
		if self.supports_ffio.load(Ordering::Acquire) {
			flags = flags.union(SataCapabilitiesFlags::FAST_FILE_IO_SUPPORTED);
		}

		let mut req = Self::construct(0x14, SataPingPacketBody::new());
		req.command_info_mut().set_user((
			self.first_read_size.load(Ordering::Acquire),
			self.first_write_size.load(Ordering::Acquire),
		));
		req.command_info_mut().set_capabilities((u32::MAX, 0));
		req.header_mut().set_flags(flags.0);
		let (_stream_id, _req_id, opt_response) = self
			.underlying_client
			.send(req, Some(timeout.unwrap_or(DEFAULT_CLIENT_TIMEOUT)))
			.await?;
		let response = opt_response.ok_or(NetworkError::ExpectedData)?;
		let pong = SataResponse::<SataPongBody>::try_from(
			response.take_body().ok_or(NetworkError::ExpectedData)?,
		)?;
		self.supports_ffio
			.store(pong.body().ffio_enabled(), Ordering::Release);
		self.supports_csr
			.store(pong.body().combined_send_recv_enabled(), Ordering::Release);

		Ok(())
	}

	/// Mark a file as 'read-only', or 'writable'.
	///
	/// In the future someday we may allow full change mode flags of unix-style
	/// flags. Unfortunately because this is based off of windows code you have
	/// only a read-only vs writable flag.
	///
	/// ## Errors
	///
	/// - If the path name is too long to be serialized.
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn change_mode(
		&self,
		path: String,
		writable: bool,
		timeout: Option<Duration>,
	) -> Result<(), CatBridgeError> {
		let resp = self
			.underlying_client
			.send(
				Self::construct(0x13, SataChangeModePacketBody::new(path, writable)?),
				Some(timeout.unwrap_or(DEFAULT_CLIENT_TIMEOUT)),
			)
			.await?
			.2
			.ok_or(NetworkError::ExpectedData)?
			.take_body()
			.ok_or(NetworkError::ExpectedData)?;

		let sata_resp = SataResponse::<SataResultCode>::try_from(resp)?;
		if sata_resp.body().0 != 0 {
			return Err(NetworkParseError::ErrorCode(sata_resp.body().0).into());
		}
		Ok(())
	}

	/// Change the user owner, and group owner of a file.
	///
	/// NOTE(mythra): this will always error when talking with a PCFS compatible
	/// server.
	///
	/// ## Errors
	///
	/// - If the path name is too long to be serialized.
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn change_owner(
		&self,
		path: String,
		owner: u32,
		group: u32,
		timeout: Option<Duration>,
	) -> Result<(), CatBridgeError> {
		let resp = self
			.underlying_client
			.send(
				Self::construct(0x12, SataChangeOwnerPacketBody::new(path, owner, group)?),
				Some(timeout.unwrap_or(DEFAULT_CLIENT_TIMEOUT)),
			)
			.await?
			.2
			.ok_or(NetworkError::ExpectedData)?
			.take_body()
			.ok_or(NetworkError::ExpectedData)?;

		let sata_resp = SataResponse::<SataResultCode>::try_from(resp)?;
		if sata_resp.body().0 != 0 {
			return Err(NetworkParseError::ErrorCode(sata_resp.body().0).into());
		}
		Ok(())
	}

	/// Create a new folder on the remote PCFS server.
	///
	/// ## Errors
	///
	/// - If the path name is too long to be serialized.
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn create_folder(
		&self,
		path: String,
		writable: bool,
		timeout: Option<Duration>,
	) -> Result<(), CatBridgeError> {
		let resp = self
			.underlying_client
			.send(
				Self::construct(0x0, SataCreateFolderPacketBody::new(path, writable)?),
				Some(timeout.unwrap_or(DEFAULT_CLIENT_TIMEOUT)),
			)
			.await?
			.2
			.ok_or(NetworkError::ExpectedData)?
			.take_body()
			.ok_or(NetworkError::ExpectedData)?;

		let sata_resp = SataResponse::<SataResultCode>::try_from(resp)?;
		if sata_resp.body().0 != 0 {
			return Err(NetworkParseError::ErrorCode(sata_resp.body().0).into());
		}
		Ok(())
	}

	/// Query a particular path for information about it.
	///
	/// In general prefer more specific query types, as opposed to this
	/// particular "grab-bag" of an interface, which will safely wrap this
	/// function and choose the right kind of return type.
	///
	/// ## Errors
	///
	/// - If the path name is too long to be serialized.
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn info_by_query(
		&self,
		path: String,
		query_type: SataQueryType,
		timeout: Option<Duration>,
	) -> Result<SataQueryResponse, CatBridgeError> {
		let resp = self
			.underlying_client
			.send(
				Self::construct(0x10, SataGetInfoByQueryPacketBody::new(path, query_type)?),
				Some(timeout.unwrap_or(DEFAULT_CLIENT_TIMEOUT)),
			)
			.await?
			.2
			.ok_or(NetworkError::ExpectedData)?
			.take_body()
			.ok_or(NetworkError::ExpectedData)?;

		let (_header, bytes) = SataResponse::<Bytes>::parse_opaque(resp)?.to_parts();

		let typed_response = match query_type {
			SataQueryType::FileCount => SataQueryResponse::try_from_small(bytes)?,
			SataQueryType::FileDetails => SataQueryResponse::try_from_fd_info(bytes)?,
			SataQueryType::FreeDiskSpace | SataQueryType::SizeOfFolder => {
				SataQueryResponse::try_from_large(bytes)?
			}
		};
		if let SataQueryResponse::ErrorCode(ec) = typed_response {
			return Err(NetworkParseError::ErrorCode(ec).into());
		}

		Ok(typed_response)
	}

	/// Get the amount of files within a particular folder.
	///
	/// ## Errors
	///
	/// - If the path name is too long to be serialized.
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn file_count(
		&self,
		path: String,
		timeout: Option<Duration>,
	) -> Result<u32, CatBridgeError> {
		let final_response = self
			.info_by_query(path, SataQueryType::FileCount, timeout)
			.await?;

		match final_response {
			SataQueryResponse::ErrorCode(_) => unreachable!("Checked in info_by_query"),
			SataQueryResponse::FDInfo(_) | SataQueryResponse::LargeSize(_) => {
				Err(SataProtocolError::WrongSataQueryResponse(final_response).into())
			}
			SataQueryResponse::SmallSize(smol) => Ok(smol),
		}
	}

	/// Get the amount of free disk space left on whatever disk the path is on.
	///
	/// ## Errors
	///
	/// - If the path name is too long to be serialized.
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn free_disk_space(
		&self,
		path: String,
		timeout: Option<Duration>,
	) -> Result<u64, CatBridgeError> {
		let final_response = self
			.info_by_query(path, SataQueryType::FreeDiskSpace, timeout)
			.await?;

		match final_response {
			SataQueryResponse::ErrorCode(_) => unreachable!("Checked in info_by_query"),
			SataQueryResponse::FDInfo(_) | SataQueryResponse::SmallSize(_) => {
				Err(SataProtocolError::WrongSataQueryResponse(final_response).into())
			}
			SataQueryResponse::LargeSize(lorg) => Ok(lorg),
		}
	}

	/// Get the total amount of space being taken by a folder.
	///
	/// ## Errors
	///
	/// - If the path name is too long to be serialized.
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn folder_size(
		&self,
		path: String,
		timeout: Option<Duration>,
	) -> Result<u64, CatBridgeError> {
		let final_response = self
			.info_by_query(path, SataQueryType::SizeOfFolder, timeout)
			.await?;

		match final_response {
			SataQueryResponse::ErrorCode(_) => unreachable!("Checked in info_by_query"),
			SataQueryResponse::FDInfo(_) | SataQueryResponse::SmallSize(_) => {
				Err(SataProtocolError::WrongSataQueryResponse(final_response).into())
			}
			SataQueryResponse::LargeSize(lorg) => Ok(lorg),
		}
	}

	/// Get the information about a particular file path.
	///
	/// ## Errors
	///
	/// - If the path name is too long to be serialized.
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn path_info(
		&self,
		path: String,
		timeout: Option<Duration>,
	) -> Result<SataFDInfo, CatBridgeError> {
		let final_response = self
			.info_by_query(path, SataQueryType::FileDetails, timeout)
			.await?;

		match final_response {
			SataQueryResponse::ErrorCode(_) => unreachable!("Checked in info_by_query"),
			SataQueryResponse::LargeSize(_) | SataQueryResponse::SmallSize(_) => {
				Err(SataProtocolError::WrongSataQueryResponse(final_response).into())
			}
			SataQueryResponse::FDInfo(info) => Ok(info),
		}
	}

	/// Remove a file or folder on the host filesystem.
	///
	/// ## Errors
	///
	/// - If the path name is too long to be serialized.
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn remove(
		&self,
		path: String,
		timeout: Option<Duration>,
	) -> Result<(), CatBridgeError> {
		let resp = self
			.underlying_client
			.send(
				Self::construct(0xE, SataRemovePacketBody::new(path)?),
				Some(timeout.unwrap_or(DEFAULT_CLIENT_TIMEOUT)),
			)
			.await?
			.2
			.ok_or(NetworkError::ExpectedData)?
			.take_body()
			.ok_or(NetworkError::ExpectedData)?;

		let sata_resp = SataResponse::<SataResultCode>::try_from(resp)?;
		if sata_resp.body().0 != 0 {
			return Err(NetworkParseError::ErrorCode(sata_resp.body().0).into());
		}
		Ok(())
	}

	/// Attempt to open a file on the remote host.
	///
	/// This will return a file handle that you can then call read, write, etc.
	/// on. You will have to call `close()` on the file handle specifically.
	///
	/// ## Errors
	///
	/// - If the path name is too long to be serialized.
	/// - If the mode setring is not formatted correctly.
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn open_file(
		&self,
		path: String,
		mode_string: String,
		timeout: Option<Duration>,
	) -> Result<SataClientFileHandle<'_>, CatBridgeError> {
		let resp = self
			.underlying_client
			.send(
				Self::construct(0x5, SataOpenFilePacketBody::new(path, mode_string)?),
				Some(timeout.unwrap_or(DEFAULT_CLIENT_TIMEOUT)),
			)
			.await?
			.2
			.ok_or(NetworkError::ExpectedData)?
			.take_body()
			.ok_or(NetworkError::ExpectedData)?;
		let fd_result = SataResponse::<SataFileDescriptorResult>::try_from(resp)?;
		let fd = match fd_result.take_body().result() {
			Ok(fd) => fd,
			Err(code) => {
				return Err(NetworkParseError::ErrorCode(code).into());
			}
		};

		Ok(SataClientFileHandle {
			file_descriptor: fd,
			underlying_client: self,
		})
	}

	/// Raw API call for doing a file read.
	///
	/// ## Errors
	///
	/// - If the path name is too long to be serialized.
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	/// - If you're requesting to read too much at once.
	async fn do_file_read(
		&self,
		block_count: u32,
		block_size: u32,
		file_descriptor: i32,
		move_to: Option<MoveToFileLocation>,
		timeout: Option<Duration>,
	) -> Result<(usize, Bytes), CatBridgeError> {
		if self.supports_ffio.load(Ordering::Acquire) {
			let mut left_to_read = block_size * block_count;

			let mut file_size = 0_usize;
			let mut final_body = BytesMut::with_capacity(
				usize::try_from(left_to_read)
					.map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?,
			);
			while left_to_read > 0 {
				// This keeps us within 'padding' range, which is easier to parse
				// responses out of.
				let read_in_this_go = std::cmp::min(
					left_to_read,
					self.first_read_size.load(Ordering::Acquire) - 0x25,
				);
				let read_in_this_go_size = usize::try_from(read_in_this_go)
					.map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?;
				// For FFIO our normal nagle split doesn't really work well.
				let (_stream_id, _req_id, opt_response) = self
					.underlying_client
					.send_with_read_amount(
						Self::construct(
							0x6,
							SataReadFilePacketBody::new(
								1,
								read_in_this_go,
								file_descriptor,
								move_to,
							),
						),
						Some(timeout.unwrap_or(DEFAULT_CLIENT_TIMEOUT)),
						// 0x20 emptied PCFS header, 0x4 final file length + (block_size * block_count)
						0x20_usize + 0x4_usize + read_in_this_go_size,
					)
					.await?;

				let mut full_body = opt_response
					.ok_or(NetworkError::ExpectedData)?
					.take_body()
					.ok_or(NetworkError::ExpectedData)?;
				// Remove 'blank' header.
				full_body.advance(0x20);
				if file_size == 0 {
					file_size = usize::try_from(full_body.get_u32()).unwrap_or(usize::MAX);
				}
				final_body.extend(full_body);
				left_to_read -= read_in_this_go;
			}

			Ok((file_size, final_body.freeze()))
		} else {
			todo!("Implement non-FFIO file support.")
		}
	}

	async fn do_file_write(
		&self,
		block_count: u32,
		block_size: u32,
		file_descriptor: i32,
		move_to: Option<MoveToFileLocation>,
		raw_trusted_data: Bytes,
		timeout: Option<Duration>,
	) -> Result<(), CatBridgeError> {
		if self.supports_ffio.load(Ordering::Acquire) {
			let base_req_bytes = Bytes::from(Self::construct(
				0x7,
				SataWriteFilePacketBody::new(block_count, block_size, file_descriptor, move_to),
			));
			let mut final_req =
				BytesMut::with_capacity(base_req_bytes.len() + raw_trusted_data.len());
			final_req.extend(base_req_bytes);
			final_req.extend(raw_trusted_data);

			let resp = self
				.underlying_client
				.send(final_req, Some(timeout.unwrap_or(DEFAULT_CLIENT_TIMEOUT)))
				.await?
				.2
				.ok_or(NetworkError::ExpectedData)?
				.take_body()
				.ok_or(NetworkError::ExpectedData)?;

			let sata_resp = SataResponse::<SataResultCode>::try_from(resp)?;
			if sata_resp.body().0 != 0 {
				return Err(NetworkParseError::ErrorCode(sata_resp.body().0).into());
			}
			Ok(())
		} else {
			todo!("Implement non-FFIO file support.")
		}
	}

	async fn stat_file(
		&self,
		file_descriptor: i32,
		timeout: Option<Duration>,
	) -> Result<SataFDInfo, CatBridgeError> {
		let resp = self
			.underlying_client
			.send(
				Self::construct(0xB, SataStatFilePacketBody::new(file_descriptor)),
				Some(timeout.unwrap_or(DEFAULT_CLIENT_TIMEOUT)),
			)
			.await?
			.2
			.ok_or(NetworkError::ExpectedData)?
			.take_body()
			.ok_or(NetworkError::ExpectedData)?;

		let (_header, bytes) = SataResponse::<Bytes>::parse_opaque(resp)?.to_parts();
		let typed_response = SataQueryResponse::try_from_fd_info(bytes)?;
		if let SataQueryResponse::ErrorCode(ec) = typed_response {
			return Err(NetworkParseError::ErrorCode(ec).into());
		}
		match typed_response {
			SataQueryResponse::FDInfo(info) => Ok(info),
			_ => unreachable!("Not reachable from try_from_fd_info"),
		}
	}

	async fn do_file_move(
		&self,
		file_descriptor: i32,
		move_to: MoveToFileLocation,
		timeout: Option<Duration>,
	) -> Result<(), CatBridgeError> {
		let resp = self
			.underlying_client
			.send(
				Self::construct(
					0x9,
					SataSetFilePositionPacketBody::new(file_descriptor, move_to),
				),
				Some(timeout.unwrap_or(DEFAULT_CLIENT_TIMEOUT)),
			)
			.await?
			.2
			.ok_or(NetworkError::ExpectedData)?
			.take_body()
			.ok_or(NetworkError::ExpectedData)?;

		let sata_resp = SataResponse::<SataResultCode>::try_from(resp)?;
		if sata_resp.body().0 != 0 {
			return Err(NetworkParseError::ErrorCode(sata_resp.body().0).into());
		}
		Ok(())
	}

	async fn close_file(
		&self,
		file_descriptor: i32,
		timeout: Option<Duration>,
	) -> Result<(), CatBridgeError> {
		let resp = self
			.underlying_client
			.send(
				Self::construct(0xD, SataCloseFilePacketBody::new(file_descriptor)),
				Some(timeout.unwrap_or(DEFAULT_CLIENT_TIMEOUT)),
			)
			.await?
			.2
			.ok_or(NetworkError::ExpectedData)?
			.take_body()
			.ok_or(NetworkError::ExpectedData)?;

		let sata_resp = SataResponse::<SataResultCode>::try_from(resp)?;
		if sata_resp.body().0 != 0 {
			return Err(NetworkParseError::ErrorCode(sata_resp.body().0).into());
		}
		Ok(())
	}

	async fn read_folder(
		&self,
		folder_descriptor: i32,
		timeout: Option<Duration>,
	) -> Result<Option<(SataFDInfo, String)>, CatBridgeError> {
		let resp = self
			.underlying_client
			.send(
				Self::construct(0x2, SataReadFolderPacketBody::new(folder_descriptor)),
				Some(timeout.unwrap_or(DEFAULT_CLIENT_TIMEOUT)),
			)
			.await?
			.2
			.ok_or(NetworkError::ExpectedData)?
			.take_body()
			.ok_or(NetworkError::ExpectedData)?;

		let sata_resp = SataResponse::<DirectoryItemResponse>::try_from(resp)?;
		let directory_item = sata_resp.take_body();
		if !directory_item.is_successful() {
			return Err(NetworkParseError::ErrorCode(directory_item.return_code()).into());
		}
		Ok(directory_item.take_file_info())
	}

	async fn rewind_folder(
		&self,
		folder_descriptor: i32,
		timeout: Option<Duration>,
	) -> Result<(), CatBridgeError> {
		let resp = self
			.underlying_client
			.send(
				Self::construct(0x3, SataRewindFolderPacketBody::new(folder_descriptor)),
				Some(timeout.unwrap_or(DEFAULT_CLIENT_TIMEOUT)),
			)
			.await?
			.2
			.ok_or(NetworkError::ExpectedData)?
			.take_body()
			.ok_or(NetworkError::ExpectedData)?;

		let sata_resp = SataResponse::<SataResultCode>::try_from(resp)?;
		if sata_resp.body().0 != 0 {
			return Err(NetworkParseError::ErrorCode(sata_resp.body().0).into());
		}
		Ok(())
	}

	async fn close_folder(
		&self,
		folder_descriptor: i32,
		timeout: Option<Duration>,
	) -> Result<(), CatBridgeError> {
		let resp = self
			.underlying_client
			.send(
				Self::construct(0x4, SataCloseFolderPacketBody::new(folder_descriptor)),
				Some(timeout.unwrap_or(DEFAULT_CLIENT_TIMEOUT)),
			)
			.await?
			.2
			.ok_or(NetworkError::ExpectedData)?
			.take_body()
			.ok_or(NetworkError::ExpectedData)?;

		let sata_resp = SataResponse::<SataResultCode>::try_from(resp)?;
		if sata_resp.body().0 != 0 {
			return Err(NetworkParseError::ErrorCode(sata_resp.body().0).into());
		}
		Ok(())
	}

	/// Construct a new sata request with a series of default values.
	#[must_use]
	fn construct<InnerTy: Into<Bytes>>(command: u32, body: InnerTy) -> SataRequest<Bytes> {
		let ci = SataCommandInfo::new((0, 0), (0, 0), command);
		let body: Bytes = body.into();
		let mut header = SataPacketHeader::new(0);
		header.set_data_len(0x14_u32 + u32::try_from(body.len()).unwrap_or(u32::MAX));

		SataRequest::new(header, ci, body)
	}
}

/// A file that is actively open on an existing sata client.
///
/// This is called with [`SataClient::open_file`], and ensures that methods
/// like "reading a file", can only ever be done on an open file handle.
#[derive(Debug, Valuable)]
pub struct SataClientFileHandle<'client> {
	/// The actual open file descriptor.
	file_descriptor: i32,
	/// The Sata Client to use.
	underlying_client: &'client SataClient,
}

impl SataClientFileHandle<'_> {
	/// Close this file, and ensure it can't be used anymore.
	///
	/// ## Errors
	///
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn close(self, timeout: Option<Duration>) -> Result<(), CatBridgeError> {
		self.underlying_client
			.close_file(self.file_descriptor, timeout)
			.await
	}

	/// Read N amounts of bytes from a file.
	///
	/// You can optionally move the file pointer around before doing the
	/// read. This will return the total file length, along with the bytes
	/// that you actually read.
	///
	/// ## Errors
	///
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn read_file(
		&self,
		amount: usize,
		move_to: Option<MoveToFileLocation>,
		timeout: Option<Duration>,
	) -> Result<(usize, Bytes), CatBridgeError> {
		let (block_size, block_len) = Self::calculate_ideal_block_size_count(amount);

		self.underlying_client
			.do_file_read(
				block_len,
				block_size,
				self.file_descriptor,
				move_to,
				timeout,
			)
			.await
	}

	/// Get information about the current open file.
	///
	/// ## Errors
	///
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn stat(&self, timeout: Option<Duration>) -> Result<SataFDInfo, CatBridgeError> {
		self.underlying_client
			.stat_file(self.file_descriptor, timeout)
			.await
	}

	/// Move to a new location within this file.
	///
	/// ## Errors
	///
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn move_to(
		&self,
		move_to: MoveToFileLocation,
		timeout: Option<Duration>,
	) -> Result<(), CatBridgeError> {
		self.underlying_client
			.do_file_move(self.file_descriptor, move_to, timeout)
			.await
	}

	/// Write N amounts of bytes from a file.
	///
	/// You can optionally move the file pointer around before doing the
	/// write.
	///
	/// *note: in cases of extreme latency a file write may succeed, but reutrn
	/// failure because we could not parse the response in the timeout.*
	///
	/// ## Errors
	///
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn write_file(
		&self,
		to_write: Bytes,
		move_to: Option<MoveToFileLocation>,
		timeout: Option<Duration>,
	) -> Result<(), CatBridgeError> {
		let (block_size, block_len) = Self::calculate_ideal_block_size_count(to_write.len());

		self.underlying_client
			.do_file_write(
				block_len,
				block_size,
				self.file_descriptor,
				move_to,
				to_write,
				timeout,
			)
			.await
	}

	fn calculate_ideal_block_size_count(amount: usize) -> (u32, u32) {
		if amount < 512 {
			(u32::try_from(amount).expect("unreachable()"), 1)
		} else if amount.is_multiple_of(512) {
			(512, u32::try_from(amount / 512).unwrap_or(u32::MAX))
		} else {
			let mut count = 511;
			while !amount.is_multiple_of(count) {
				count -= 1;
			}

			(
				u32::try_from(count).expect("unreachable()"),
				u32::try_from(amount / count).unwrap_or(u32::MAX),
			)
		}
	}
}

/// A folder that is actively open on an existing sata client.
///
/// This is called with [`SataClient::open_folder`], and ensures that methods
/// like "reading a directory", can only ever be done on an open folder handle.
#[derive(Debug, Valuable)]
pub struct SataClientFolderHandle<'client> {
	/// The folder handle and descriptor.
	folder_descriptor: i32,
	/// The underlying sata client to call methods on.
	underlying_client: &'client SataClient,
}

impl SataClientFolderHandle<'_> {
	/// Close this folder, and ensure it can't be used anymore.
	///
	/// ## Errors
	///
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn close(self, timeout: Option<Duration>) -> Result<(), CatBridgeError> {
		self.underlying_client
			.close_folder(self.folder_descriptor, timeout)
			.await
	}

	/// Read the next item within a folder.
	///
	/// This will return the next file name/file info if there is another item in
	/// this directory.
	///
	/// ## Errors
	///
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn next_in_folder(
		&self,
		timeout: Option<Duration>,
	) -> Result<Option<(SataFDInfo, String)>, CatBridgeError> {
		self.underlying_client
			.read_folder(self.folder_descriptor, timeout)
			.await
	}

	/// Rewind the directory iterator by one (so next in folder returns the
	/// previous item it returned).
	///
	/// ## Errors
	///
	/// - If we cannot send the request to the PCFS Sata server.
	/// - If we cannot receive a response from the PCFS Sata server.
	/// - If we cannot parse the response from the PCFS Sata server.
	/// - If the return code of the response was not successful.
	pub async fn rewind_iterator(&self, timeout: Option<Duration>) -> Result<(), CatBridgeError> {
		self.underlying_client
			.rewind_folder(self.folder_descriptor, timeout)
			.await
	}
}
// Folder Open Handle (OpenFolder):
//   - CloseFolder
