//! Client implementation for SATA over PCFS.

/*
use crate::{
	errors::{CatBridgeError, NetworkError, NetworkParseError},
	fsemul::pcfs::{
		errors::PCFSApiError,
		sata::proto::{
			DirectoryItemResponse, MoveToFileLocation, PCFSSataFdInfo, PCFSSataQueryResponse,
			PCFSSataQueryType, SataCapabilitiesFlags, SataChangeModePacketBody,
			SataChangeOwnerPacketBody, SataCloseFilePacketBody, SataCloseFolderPacketBody,
			SataCommandInfo, SataCreateFolderPacketBody, SataGetInfoByQueryPacketBody,
			SataOpenFilePacketBody, SataOpenFolderPacketBody, SataPacketHeader, SataPingPacketBody,
			SataPongBody, SataReadFilePacketBody, SataReadFolderPacketBody, SataRemovePacketBody,
			SataRewindFolderPacketBody, SataStatFilePacketBody, SataWriteFilePacketBody,
			construct_sata_request,
		},
	},
};
use bytes::{Buf, Bytes, BytesMut};
use std::time::Duration;
use tokio::{
	io::{AsyncReadExt, AsyncWriteExt},
	net::{TcpStream, ToSocketAddrs},
	time::sleep,
};

/// A connection to a SATA PCFS server.
#[derive(Debug)]
pub struct PCFSSataClient {
	/// If we're actively supporting "CSR", or "Combined Send/Recv".
	supports_csr: bool,
	/// If we're acctively supporting "FFIO", or "Fast File I/O".
	supports_ffio: bool,
	/// The connection to the server.
	underlying_stream: TcpStream,
}

impl PCFSSataClient {
	/// Attmempt to connect to a real PCFS Sata Server implementation.
	///
	/// ## Errors
	///
	/// If for some reason we cannot open a connection to the PCFS Sata Server
	/// due to some underlying network error or condition.
	pub async fn connect_to<AddrTy: ToSocketAddrs>(
		address: AddrTy,
		supports_csr: bool,
		supports_ffio: bool,
	) -> Result<Self, CatBridgeError> {
		let conn = TcpStream::connect(address)
			.await
			.map_err(NetworkError::IO)?;
		conn.set_nodelay(true).map_err(NetworkError::IO)?;

		let mut this = Self {
			supports_csr,
			supports_ffio,
			underlying_stream: conn,
		};
		// Synchronize server with our supports csr/ffio state.
		this.ping().await?;
		Ok(this)
	}

	/// Swap the current state of CSR, and FFIO support.
	///
	/// This will require sending a PING to the server, and get a response
	/// back. If the ping errors out at all, then the CSR/FFIO flag will not
	/// change at all.
	///
	/// ## Errors
	///
	/// - If we cannot successfully send a ping to the server.
	pub async fn set_supports_csr_and_ffio(
		&mut self,
		supports_csr: bool,
		supports_ffio: bool,
	) -> Result<(), CatBridgeError> {
		let old_csr = self.supports_csr;
		let old_ffio = self.supports_ffio;

		self.supports_csr = supports_csr;
		self.supports_ffio = supports_ffio;
		let result = self.ping().await;
		// Keep us in the same state.
		if result.is_err() {
			self.supports_csr = old_csr;
			self.supports_ffio = old_ffio;
		}
		result
	}

	/// Swap the current state of CSR support.
	///
	/// This will require sending a PING to the server, and get a response
	/// back. If the ping errors out at all, then the CSR flag will not change
	/// at all.
	///
	/// ## Errors
	///
	/// - If we cannot successfully send a ping to the server.
	pub async fn set_supports_csr(&mut self, supports_csr: bool) -> Result<(), CatBridgeError> {
		let old = self.supports_csr;
		self.supports_csr = supports_csr;
		let result = self.ping().await;
		// Keep us in the same state.
		if result.is_err() {
			self.supports_csr = old;
		}
		result
	}

	/// Swap the current state of FFIO support.
	///
	/// This will require sending a PING to the server, and get a response
	/// back. If the ping errors out at all, then the FFIO flag will not change
	/// at all.
	///
	/// ## Errors
	///
	/// - If we cannot successfully send a ping to the server.
	pub async fn set_supports_ffio(&mut self, supports_ffio: bool) -> Result<(), CatBridgeError> {
		let old = self.supports_ffio;
		self.supports_ffio = supports_ffio;
		let result = self.ping().await;
		// Keep us in the same state.
		if result.is_err() {
			self.supports_ffio = old;
		}
		result
	}

	/// Change the mode of a particular file, since this is based on Windows
	/// files ultimately, the protocol only allows for changing the read/write
	/// flag.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	/// - If we do not get a successful response back.
	pub async fn change_mode(
		&mut self,
		path: String,
		set_write_mode: bool,
	) -> Result<(), CatBridgeError> {
		Ok(Self::validate_pcfs_rc(
			"PCFSSataChangeModeResponse",
			self.change_mode_raw(path, set_write_mode).await?,
		)?)
	}

	/// Change the mode of a particular file, since this is based on Windows
	/// files ultimately, the protocol only allows for changing the read/write
	/// flag.
	///
	/// This will return the raw series of bytes we tried to peek from the
	/// network. This may not be any valid packet, or may not be what we're
	/// expecting.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	pub async fn change_mode_raw(
		&mut self,
		path: String,
		set_write_mode: bool,
	) -> Result<Bytes, CatBridgeError> {
		self.underlying_stream
			.write_all(&construct_sata_request(
				&SataPacketHeader::new(0),
				&SataCommandInfo::new((0, 0), (0, 0), 0x13),
				0,
				SataChangeModePacketBody::new(path, set_write_mode)?,
			)?)
			.await
			.map_err(NetworkError::IO)?;

		self.try_recv_data()
			.await?
			.ok_or(NetworkError::ExpectedData.into())
	}

	/// Change the owner of a particular file.
	///
	/// This will always fail on a compatible implementation as windows doesn't
	/// have the same concept of file owners.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	/// - If we do not get a successful response back.
	pub async fn change_owner(
		&mut self,
		path: String,
		uid: u32,
		gid: u32,
	) -> Result<(), CatBridgeError> {
		Ok(Self::validate_pcfs_rc(
			"PCFSSataChangeOwnerResponse",
			self.change_owner_raw(path, uid, gid).await?,
		)?)
	}

	/// Change the owner of a particular file.
	///
	/// This will always fail on a compatible implementation as windows doesn't
	/// have the same concept of file owners.
	///
	/// This will return the raw series of bytes we tried to peek from the
	/// network. This may not be any valid packet, or may not be what we're
	/// expecting.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	pub async fn change_owner_raw(
		&mut self,
		path: String,
		uid: u32,
		gid: u32,
	) -> Result<Bytes, CatBridgeError> {
		self.underlying_stream
			.write_all(&construct_sata_request(
				&SataPacketHeader::new(0),
				&SataCommandInfo::new((0, 0), (0, 0), 0x12),
				0,
				SataChangeOwnerPacketBody::new(path, uid, gid)?,
			)?)
			.await
			.map_err(NetworkError::IO)?;

		self.try_recv_data()
			.await?
			.ok_or(NetworkError::ExpectedData.into())
	}

	/// Close an existing open file handle.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	/// - If we do not get a successful response back.
	pub async fn close_file(&mut self, fd: i32) -> Result<(), CatBridgeError> {
		Ok(Self::validate_pcfs_rc(
			"PCFSSataCloseFileResponse",
			self.close_file_raw(fd).await?,
		)?)
	}

	/// Close an existing open file handle.
	///
	/// This will return the raw series of bytes we tried to peek from the
	/// network. This may not be any valid packet, or may not be what we're
	/// expecting.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	pub async fn close_file_raw(&mut self, fd: i32) -> Result<Bytes, CatBridgeError> {
		self.underlying_stream
			.write_all(&construct_sata_request(
				&SataPacketHeader::new(0),
				&SataCommandInfo::new((0, 0), (0, 0), 0xD),
				0,
				SataCloseFilePacketBody::new(fd),
			)?)
			.await
			.map_err(NetworkError::IO)?;

		self.try_recv_data()
			.await?
			.ok_or(NetworkError::ExpectedData.into())
	}

	/// Close an existing open folder handle.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	/// - If we do not get a successful response back.
	pub async fn close_folder(&mut self, fd: i32) -> Result<(), CatBridgeError> {
		Ok(Self::validate_pcfs_rc(
			"PCFSSataCloseFolderResponse",
			self.close_folder_raw(fd).await?,
		)?)
	}

	/// Close an existing open folder handle.
	///
	/// This will return the raw series of bytes we tried to peek from the
	/// network. This may not be any valid packet, or may not be what we're
	/// expecting.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	pub async fn close_folder_raw(&mut self, fd: i32) -> Result<Bytes, CatBridgeError> {
		self.underlying_stream
			.write_all(&construct_sata_request(
				&SataPacketHeader::new(0),
				&SataCommandInfo::new((0, 0), (0, 0), 0x4),
				0,
				SataCloseFolderPacketBody::new(fd),
			)?)
			.await
			.map_err(NetworkError::IO)?;

		self.try_recv_data()
			.await?
			.ok_or(NetworkError::ExpectedData.into())
	}

	/// Create a Directory.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	/// - If we do not get a successful response back.
	pub async fn create_directory(
		&mut self,
		path: String,
		set_write_mode: bool,
	) -> Result<(), CatBridgeError> {
		Ok(Self::validate_pcfs_rc(
			"PCFSSataCreateDirectoryResponse",
			self.create_directory_raw(path, set_write_mode).await?,
		)?)
	}

	/// Create a directory.
	///
	/// This will return the raw series of bytes we tried to peek from the
	/// network. This may not be any valid packet, or may not be what we're
	/// expecting.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	pub async fn create_directory_raw(
		&mut self,
		path: String,
		set_write_mode: bool,
	) -> Result<Bytes, CatBridgeError> {
		self.underlying_stream
			.write_all(&construct_sata_request(
				&SataPacketHeader::new(0),
				&SataCommandInfo::new((0, 0), (0, 0), 0x0),
				0,
				SataCreateFolderPacketBody::new(path, set_write_mode)?,
			)?)
			.await
			.map_err(NetworkError::IO)?;

		self.try_recv_data()
			.await?
			.ok_or(NetworkError::ExpectedData.into())
	}

	/// Get the information related to a specific file.
	///
	/// There are multiple query types that can return various types of
	/// information. This is a generic catch all, but there are more specific
	/// methods that return specific types, these are:
	///
	/// - [`Self::get_info_disk_space_for_path`]
	/// - [`Self::get_info_folder_space`]
	/// - [`Self::get_info_file_count`]
	/// - [`Self::get_info_stat`]
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	/// - If we do not get a successful response back.
	pub async fn get_info(
		&mut self,
		path: String,
		query_type: PCFSSataQueryType,
	) -> Result<PCFSSataQueryResponse, CatBridgeError> {
		let result = self.get_info_raw(path, query_type).await?;

		match query_type {
			PCFSSataQueryType::FreeDiskSpace => Ok(PCFSSataQueryResponse::try_from_large(result)?),
			PCFSSataQueryType::SizeOfFolder => Ok(PCFSSataQueryResponse::try_from_large(result)?),
			PCFSSataQueryType::FileCount => Ok(PCFSSataQueryResponse::try_from_small(result)?),
			PCFSSataQueryType::FileDetails => Ok(PCFSSataQueryResponse::try_from_fd_info(result)?),
		}
	}

	/// Get the amount of disk space for the disk that is storing a particular
	/// path.
	///
	/// For a more generic info query type see [`Self::get_info`].
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	/// - If we do not get a successful response back.
	pub async fn get_info_disk_space_for_path(
		&mut self,
		path: String,
	) -> Result<u64, CatBridgeError> {
		let result = PCFSSataQueryResponse::try_from_large(
			self.get_info_raw(path, PCFSSataQueryType::FreeDiskSpace)
				.await?,
		)?;

		if let PCFSSataQueryResponse::LargeSize(lorg) = result {
			Ok(lorg)
		} else {
			unreachable!("`try_from_large` should always return large size")
		}
	}

	/// Get the amount of space a folder takes up on disk.
	///
	/// For a more generic info query type see [`Self::get_info`].
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	/// - If we do not get a successful response back.
	pub async fn get_info_folder_space(&mut self, path: String) -> Result<u64, CatBridgeError> {
		let result = PCFSSataQueryResponse::try_from_large(
			self.get_info_raw(path, PCFSSataQueryType::SizeOfFolder)
				.await?,
		)?;

		if let PCFSSataQueryResponse::LargeSize(lorg) = result {
			Ok(lorg)
		} else {
			unreachable!("`try_from_large` should always return large size")
		}
	}

	/// Get the count of files within a particular directory.
	///
	/// For a more generic info query type see [`Self::get_info`].
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	/// - If we do not get a successful response back.
	pub async fn get_info_file_count(&mut self, path: String) -> Result<u32, CatBridgeError> {
		let result = PCFSSataQueryResponse::try_from_small(
			self.get_info_raw(path, PCFSSataQueryType::SizeOfFolder)
				.await?,
		)?;

		if let PCFSSataQueryResponse::SmallSize(smol) = result {
			Ok(smol)
		} else {
			unreachable!("`try_from_small` should always return small size")
		}
	}

	/// Get the information about a particular path.
	///
	/// For a more generic info query type see [`Self::get_info`].
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	/// - If we do not get a successful response back.
	pub async fn get_info_stat(&mut self, path: String) -> Result<PCFSSataFdInfo, CatBridgeError> {
		let result = PCFSSataQueryResponse::try_from_fd_info(
			self.get_info_raw(path, PCFSSataQueryType::FileDetails)
				.await?,
		)?;

		if let PCFSSataQueryResponse::FDInfo(info) = result {
			Ok(info)
		} else {
			unreachable!("`try_from_fd_info` should always return fd info")
		}
	}

	/// Get the information about a particular path that exists on disc.
	///
	/// This will return the raw series of bytes we tried to peek from the
	/// network. This may not be any valid packet, or may not be what we're
	/// expecting.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	pub async fn get_info_raw(
		&mut self,
		path: String,
		query_type: PCFSSataQueryType,
	) -> Result<Bytes, CatBridgeError> {
		self.underlying_stream
			.write_all(&construct_sata_request(
				&SataPacketHeader::new(0),
				&SataCommandInfo::new((0, 0), (0, 0), 0x10),
				0,
				SataGetInfoByQueryPacketBody::new(path, query_type)?,
			)?)
			.await
			.map_err(NetworkError::IO)?;

		self.try_recv_data()
			.await?
			.ok_or(NetworkError::ExpectedData.into())
	}

	/// Open a file on the existing remote end returning a file handle.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	/// - If we cannot parse the response body.
	pub async fn open_file(
		&mut self,
		path: String,
		mode_string: String,
	) -> Result<i32, CatBridgeError> {
		Ok(Self::get_pcfs_fd_with_rc(
			"PCFSSataOpenFileResponse",
			self.open_file_raw(path, mode_string).await?,
		)?)
	}

	/// Open a file on the existing remote end returning a file handle.
	///
	/// This will return the raw series of bytes we tried to peek from the
	/// network. This may not be any valid packet, or may not be what we're
	/// expecting.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	pub async fn open_file_raw(
		&mut self,
		path: String,
		mode_string: String,
	) -> Result<Bytes, CatBridgeError> {
		self.underlying_stream
			.write_all(&construct_sata_request(
				&SataPacketHeader::new(0),
				&SataCommandInfo::new((0, 0), (0, 0), 0x5),
				0,
				SataOpenFilePacketBody::new(path, mode_string)?,
			)?)
			.await
			.map_err(NetworkError::IO)?;

		self.try_recv_data()
			.await?
			.ok_or(NetworkError::ExpectedData.into())
	}

	/// Open a folder on the existing remote end returning a file handle.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	/// - If we cannot parse the response body.
	pub async fn open_folder(&mut self, path: String) -> Result<i32, CatBridgeError> {
		Ok(Self::get_pcfs_fd_with_rc(
			"PCFSSataOpenFolderResponse",
			self.open_folder_raw(path).await?,
		)?)
	}

	/// Open a folder on the existing remote end returning a file handle.
	///
	/// This will return the raw series of bytes we tried to peek from the
	/// network. This may not be any valid packet, or may not be what we're
	/// expecting.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	pub async fn open_folder_raw(&mut self, path: String) -> Result<Bytes, CatBridgeError> {
		self.underlying_stream
			.write_all(&construct_sata_request(
				&SataPacketHeader::new(0),
				&SataCommandInfo::new((0, 0), (0, 0), 0x1),
				0,
				SataOpenFolderPacketBody::new(path)?,
			)?)
			.await
			.map_err(NetworkError::IO)?;

		self.try_recv_data()
			.await?
			.ok_or(NetworkError::ExpectedData.into())
	}

	/// Send a ping, and update CSR/FFIO.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot read a packet from the stream.
	/// - If we cannot parse the response body.
	pub async fn ping(&mut self) -> Result<(), CatBridgeError> {
		let packet = self.ping_raw().await?;
		if packet.len() < 0x20 {
			return Err(NetworkParseError::NotEnoughData(
				"PCFSSataPong",
				0x20,
				packet.len(),
				packet,
			)
			.into());
		}
		let body = SataPongBody::try_from(packet.slice(0x20..))?;
		if !body.ffio_enabled() {
			self.supports_ffio = false;
		}
		if !body.combined_send_recv_enabled() {
			self.supports_csr = false;
		}

		Ok(())
	}

	/// Send a ping, and update CSR/FFIO.
	///
	/// This will return the raw series of bytes we tried to peek from the
	/// network. This may not be any valid packet, or may not be what we're
	/// expecting.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot read a packet from the stream.
	pub async fn ping_raw(&mut self) -> Result<Bytes, CatBridgeError> {
		let mut flags = SataCapabilitiesFlags::empty();
		if self.supports_csr {
			flags = flags.union(SataCapabilitiesFlags::COMBINED_SEND_RECV_SUPPORTED);
		}
		if self.supports_ffio {
			flags = flags.union(SataCapabilitiesFlags::FAST_FILE_IO_SUPPORTED);
		}

		self.underlying_stream
			.write_all(&construct_sata_request(
				&SataPacketHeader::new(0),
				&SataCommandInfo::new((0, 0), (0, 0), 0x14),
				flags.0,
				SataPingPacketBody::new(),
			)?)
			.await
			.map_err(NetworkError::IO)?;

		self.try_recv_data()
			.await?
			.ok_or(NetworkError::ExpectedData.into())
	}

	/// Read the next item in a directory.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot read a packet from the stream.
	/// - If we cannot parse the response body.
	pub async fn read_directory(
		&mut self,
		file_descriptor: i32,
	) -> Result<Option<(PCFSSataFdInfo, String)>, CatBridgeError> {
		let response =
			DirectoryItemResponse::try_from(self.read_directory_raw(file_descriptor).await?)?;

		Ok(response.take_file_info())
	}

	/// Read the next item in a directory.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot read a packet from the stream.
	pub async fn read_directory_raw(
		&mut self,
		file_descriptor: i32,
	) -> Result<Bytes, CatBridgeError> {
		self.underlying_stream
			.write_all(&construct_sata_request(
				&SataPacketHeader::new(0),
				&SataCommandInfo::new((0, 0), (0, 0), 0x2),
				0,
				Bytes::from(SataReadFolderPacketBody::new(file_descriptor)),
			)?)
			.await
			.map_err(NetworkError::IO)?;

		self.try_recv_data()
			.await?
			.ok_or(NetworkError::ExpectedData.into())
	}

	/// Read the next set of bytes from a file.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot read a packet from the stream.
	/// - If we cannot parse the response body.
	pub async fn read_file(
		&mut self,
		block_count: u32,
		block_size: u32,
		file_descriptor: i32,
		move_to: Option<MoveToFileLocation>,
	) -> Result<Bytes, CatBridgeError> {
		let raw_buff = self
			.read_file_raw(block_count, block_size, file_descriptor, move_to)
			.await?;

		if self.supports_ffio {
			Ok(raw_buff.slice(0x24..))
		} else {
			todo!("Implement Non-FFIO SUPPORT")
		}
	}

	/// Read the next set of bytes from a file.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot read a packet from the stream.
	pub async fn read_file_raw(
		&mut self,
		block_count: u32,
		block_size: u32,
		file_descriptor: i32,
		move_to: Option<MoveToFileLocation>,
	) -> Result<Bytes, CatBridgeError> {
		self.underlying_stream
			.write_all(&construct_sata_request(
				&SataPacketHeader::new(0),
				&SataCommandInfo::new((0, 0), (0, 0), 0x6),
				0,
				Bytes::from(SataReadFilePacketBody::new(
					block_count,
					block_size,
					file_descriptor,
					move_to,
				)),
			)?)
			.await
			.map_err(NetworkError::IO)?;

		self.try_recv_data()
			.await?
			.ok_or(NetworkError::ExpectedData.into())
	}

	/// Remove a path on the host filesystem.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot receive a packet back from the stream.
	/// - If we cannot parse the response body.
	pub async fn remove(&mut self, path: String) -> Result<(), CatBridgeError> {
		Ok(Self::validate_pcfs_rc(
			"PCFSSataRemoveResponse",
			self.remove_raw(path).await?,
		)?)
	}

	/// Remove a file or directory froom the host filesystem.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot read a packet from the stream.
	pub async fn remove_raw(&mut self, path: String) -> Result<Bytes, CatBridgeError> {
		self.underlying_stream
			.write_all(&construct_sata_request(
				&SataPacketHeader::new(0),
				&SataCommandInfo::new((0, 0), (0, 0), 0xE),
				0,
				Bytes::from(SataRemovePacketBody::new(path)?),
			)?)
			.await
			.map_err(NetworkError::IO)?;

		self.try_recv_data()
			.await?
			.ok_or(NetworkError::ExpectedData.into())
	}

	/// Rewind a directory iterator to the beginning.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot read a packet from the stream.
	/// - If we cannot parse the response body.
	pub async fn rewind(&mut self, file_descriptor: i32) -> Result<(), CatBridgeError> {
		Ok(Self::validate_pcfs_rc(
			"PCFSSataRemoveResponse",
			self.rewind_raw(file_descriptor).await?,
		)?)
	}

	/// Rewind a directory iterator to the beginning.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot read a packet from the stream.
	pub async fn rewind_raw(&mut self, file_descriptor: i32) -> Result<Bytes, CatBridgeError> {
		self.underlying_stream
			.write_all(&construct_sata_request(
				&SataPacketHeader::new(0),
				&SataCommandInfo::new((0, 0), (0, 0), 0x3),
				0,
				Bytes::from(SataRewindFolderPacketBody::new(file_descriptor)),
			)?)
			.await
			.map_err(NetworkError::IO)?;

		self.try_recv_data()
			.await?
			.ok_or(NetworkError::ExpectedData.into())
	}

	/// Stat a particular file descriptor to get the information related to a
	/// file.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot read a packet from the stream.
	/// - If we cannot parsee the response body.
	pub async fn stat(&mut self, file_descriptor: i32) -> Result<PCFSSataFdInfo, CatBridgeError> {
		let raw_resp = self.stat_raw(file_descriptor).await?;
		Ok(PCFSSataFdInfo::try_from(raw_resp)?)
	}

	/// Stat a particular file descriptor to get the information related to a
	/// file.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot read a packet from the stream.
	pub async fn stat_raw(&mut self, file_descriptor: i32) -> Result<Bytes, CatBridgeError> {
		self.underlying_stream
			.write_all(&construct_sata_request(
				&SataPacketHeader::new(0),
				&SataCommandInfo::new((0, 0), (0, 0), 0xB),
				0,
				Bytes::from(SataStatFilePacketBody::new(file_descriptor)),
			)?)
			.await
			.map_err(NetworkError::IO)?;

		self.try_recv_data()
			.await?
			.ok_or(NetworkError::ExpectedData.into())
	}

	/// Write a series of bytes to a file that is already open on the host
	/// system.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot read a packet from the stream.
	/// - If we cannot parse the response body.
	pub async fn write(
		&mut self,
		file_descriptor: i32,
		move_to: Option<MoveToFileLocation>,
		buff: Bytes,
	) -> Result<(), CatBridgeError> {
		Ok(Self::validate_pcfs_rc(
			"PCFSSataWriteResponseBody",
			self.write_raw(file_descriptor, move_to, buff).await?,
		)?)
	}

	/// Write a series of bytes to a file that is already open on the host
	/// system.
	///
	/// ## Errors
	///
	/// - If we cannot send a request over the stream.
	/// - If we cannot read a packet from the stream.
	pub async fn write_raw(
		&mut self,
		file_descriptor: i32,
		move_to: Option<MoveToFileLocation>,
		buff: Bytes,
	) -> Result<Bytes, CatBridgeError> {
		self.underlying_stream
			.write_all(&construct_sata_request(
				&SataPacketHeader::new(0),
				&SataCommandInfo::new((0, 0), (0, 0), 0x7),
				0,
				Bytes::from(SataWriteFilePacketBody::new(
					1,
					u32::try_from(buff.len())
						.map_err(|_| PCFSApiError::PacketTooLargeForSata(buff.len()))?,
					file_descriptor,
					move_to,
				)),
			)?)
			.await
			.map_err(NetworkError::IO)?;
		self.underlying_stream
			.write_all(&buff)
			.await
			.map_err(NetworkError::IO)?;

		self.try_recv_data()
			.await?
			.ok_or(NetworkError::ExpectedData.into())
	}

	/// This will attempt to try and read data off of the network until a
	/// response hasn't been hit for 100ms.
	///
	/// This is mostly a very hacky method to be utilized when we don't know if
	/// the remote side is sending us anything, but we do want to know if it sent
	/// _something_.
	///
	/// This is mostly used in the "Scientist" classes which attempt to diff our
	/// implementation with an official cafe-sdk implementation in real time,
	/// printing out any differences in sent/received data.
	///
	/// ## Errors
	///
	/// If the underlying stream returns an error for us.
	pub async fn try_recv_data(&mut self) -> Result<Option<Bytes>, NetworkError> {
		let mut buff = BytesMut::new();
		let mut inner_buff = BytesMut::zeroed(8192);

		loop {
			tokio::select! {
			  res = self.underlying_stream.read(&mut inner_buff) => {
					let size = res.map_err(NetworkError::IO)?;
					buff.extend_from_slice(&inner_buff[..size]);
			  }
			  () = sleep(Duration::from_millis(100)) => {
					break;
			  }
			}
		}

		if buff.is_empty() {
			Ok(None)
		} else {
			Ok(Some(buff.freeze()))
		}
	}

	/// Read a response body, throwing most of the data out, just validate
	/// the first 4 bytes are 0, generally used to determine success.
	fn validate_pcfs_rc(name: &'static str, data: Bytes) -> Result<(), NetworkParseError> {
		if data.len() < 0x24 {
			return Err(NetworkParseError::NotEnoughData(
				name,
				0x24,
				data.len(),
				data,
			));
		}

		let mut body = data.slice(0x20..);
		let rc = body.get_u32();
		if rc != 0 {
			return Err(NetworkParseError::ErrorCode(rc));
		}

		Ok(())
	}

	/// Read a response body, throwing most of the data out, just validate
	/// the first 4 bytes are 0, and getting the next 4 bytes as an i32 in
	/// big endian.
	fn get_pcfs_fd_with_rc(name: &'static str, data: Bytes) -> Result<i32, NetworkParseError> {
		if data.len() < 0x28 {
			return Err(NetworkParseError::NotEnoughData(
				name,
				0x28,
				data.len(),
				data,
			));
		}

		let mut body = data.slice(0x20..);
		let rc = body.get_u32();
		if rc != 0 {
			return Err(NetworkParseError::ErrorCode(rc));
		}
		let fd = body.get_i32();

		Ok(fd)
	}
}
*/
