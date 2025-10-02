//! Process a query request for information about a particular path.
//!
//! This route accepts one of a series of query types such as getting
//! the disk space available at a particular path, the number of files in a
//! folder, etc.

use crate::{
	fsemul::{
		HostFilesystem,
		host_filesystem::{FilesystemLocation, ResolvedLocation},
		pcfs::sata::{
			proto::{
				SataFDInfo, SataGetInfoByQueryPacketBody, SataPacketHeader, SataQueryResponse,
				SataQueryType, SataRequest, SataResponse, SataStatFilePacketBody,
			},
			server::PCFSServerState,
		},
	},
	net::{
		additions::StreamID,
		server::requestable::{Body, State},
	},
};
use std::fs::read_dir;
use sysinfo::{Disk, Disks};
use tracing::{debug, field::valuable, warn};
use walkdir::WalkDir;

/// A folder is required to call this particular api.
const FOLDER_REQUIRED_ERROR: u32 = 0xFFF0_FFD7;
/// Returned when a directory is too large to walk.
const SIZE_TOO_BIG_ERROR: u32 = 0xFFF0_FFE7;
/// An error code to send when a path does not exist.
///
/// This is also used in some places that are a bit of a stretch like for
/// network shares on disk space. The path doesn't exist on a disk, so this
/// error code is used, even if it's not quite exact.
const PATH_NOT_EXIST_ERROR: u32 = 0xFFF0_FFE9;

/// Handle a Get Info Query request.
///
/// This handles getting the types of information from the filesystem.
pub async fn handle_get_info_by_query(
	State(state): State<PCFSServerState>,
	Body(request): Body<SataRequest<SataGetInfoByQueryPacketBody>>,
) -> SataResponse<SataQueryResponse> {
	let info_request = request.body();
	let header = request.header().clone();

	let fs = state.host_filesystem();
	let Ok(final_location) = fs.resolve_path(info_request.path()) else {
		return SataResponse::new(
			state.pid(),
			header,
			SataQueryResponse::ErrorCode(PATH_NOT_EXIST_ERROR),
		);
	};
	if request.command_info().user().0 == 0x1000_00FC
		&& request.command_info().user().1 == 0x1000_00FF
	{
		let cloned_location = final_location.clone();
		let ResolvedLocation::Filesystem(fs_location) = cloned_location else {
			todo!("network shares not yet implemented!")
		};
		let resolved_path = fs_location.resolved_path();

		if fs.path_allows_writes(resolved_path)
			&& resolved_path.extension().is_none()
			&& !resolved_path.exists()
		{
			// Ignore any errors, file details or otherwise will properly error out.
			_ = fs.create_directory(resolved_path);
		}
	}

	match info_request.query_type() {
		SataQueryType::FreeDiskSpace => handle_disk_space(state.pid(), header, final_location),
		SataQueryType::SizeOfFolder => handle_folder_size(state.pid(), header, final_location),
		SataQueryType::FileCount => handle_file_count(state.pid(), header, final_location),
		SataQueryType::FileDetails => {
			handle_file_info(state.pid(), header, fs, final_location).await
		}
	}
}

/// Get the file information given a particular already open fd.
///
/// ## Errors
///
/// If we cannot construct a sata response packet.
pub async fn stat_fd(
	stream: StreamID,
	State(state): State<PCFSServerState>,
	Body(request): Body<SataRequest<SataStatFilePacketBody>>,
) -> SataResponse<SataQueryResponse> {
	let body = request.body();
	let header = request.header().clone();

	let path = {
		let Some(entry) = state
			.host_filesystem()
			.get_file(body.file_descriptor(), Some(stream.to_raw()))
			.await
		else {
			debug!(
				packet.fd = body.file_descriptor(),
				packet.typ = "GetInfoByQueryPacketBody::stat_fd",
				"Processing stat of already open fd",
			);

			return SataResponse::new(
				state.pid(),
				header,
				SataQueryResponse::ErrorCode(PATH_NOT_EXIST_ERROR),
			);
		};
		entry.2.clone()
	};

	handle_file_info(
		state.pid(),
		header,
		state.host_filesystem(),
		ResolvedLocation::Filesystem(FilesystemLocation::new(path.clone(), path, true)),
	)
	.await
}

/// Get the total amount of free disk space a path would be in.
fn handle_disk_space(
	pid: u32,
	request_header: SataPacketHeader,
	location: ResolvedLocation,
) -> SataResponse<SataQueryResponse> {
	// The file needs to know which disk we're on.
	//
	// So the path needs to exist, or one of it's parent paths do, and it
	// needs to not be on the network....
	let ResolvedLocation::Filesystem(fs_location) = location else {
		debug!(
			packet.location = valuable(&location),
			packet.typ = "PCFSSrvGetInfo",
			packet.sub_type = "handle_disk_space",
			"Failed to resolve path!",
		);

		// Network locations cannot have a disk space to measure.
		return SataResponse::new(
			pid,
			request_header,
			SataQueryResponse::ErrorCode(PATH_NOT_EXIST_ERROR),
		);
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
			if let Some(other_potential_source) = disk_holding_path
				&& other_potential_source.mount_point().components().count()
					> potential_disk.mount_point().components().count()
			{
				should_insert = false;
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
		return SataResponse::new(
			pid,
			request_header,
			SataQueryResponse::ErrorCode(PATH_NOT_EXIST_ERROR),
		);
	};

	SataResponse::new(
		pid,
		request_header,
		SataQueryResponse::LargeSize(disk.available_space()),
	)
}

/// Get the total size of a folder, must be able to fit within a [`u32`].
fn handle_folder_size(
	pid: u32,
	request_header: SataPacketHeader,
	location: ResolvedLocation,
) -> SataResponse<SataQueryResponse> {
	let ResolvedLocation::Filesystem(fs_location) = location else {
		todo!("network shares not yet implemented!")
	};

	// This means our path doesn't exist, so we can't iterate over our folder
	// because it doesn't exist.
	if !fs_location.canonicalized_is_exact() {
		debug!(
			packet.location = valuable(&fs_location),
			packet.typ = "PCFSSrvGetInfo",
			packet.sub_type = "handle_folder_size",
			"Failed to resolve path!",
		);

		return SataResponse::new(
			pid,
			request_header,
			SataQueryResponse::ErrorCode(PATH_NOT_EXIST_ERROR),
		);
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

	SataResponse::new(
		pid,
		request_header,
		SataQueryResponse::LargeSize(total_size),
	)
}

/// Get the total amount of files in a directory _non-recursively_.
fn handle_file_count(
	pid: u32,
	request_header: SataPacketHeader,
	location: ResolvedLocation,
) -> SataResponse<SataQueryResponse> {
	let ResolvedLocation::Filesystem(fs_location) = location else {
		todo!("network shares not yet implemented!")
	};
	// This means our path doesn't exist, so we can't iterate over our folder
	// because it doesn't exist.
	if !fs_location.canonicalized_is_exact() {
		debug!(
			packet.location = valuable(&fs_location),
			packet.typ = "PCFSSrvGetInfo",
			packet.sub_type = "handle_file_count",
			"Failed to resolve path!",
		);

		return SataResponse::new(
			pid,
			request_header,
			SataQueryResponse::ErrorCode(PATH_NOT_EXIST_ERROR),
		);
	}

	if !fs_location.resolved_path().is_dir() {
		debug!(
			packet.typ = "PCFSSrvGetInfo",
			packet.sub_type = "handle_folder_size",
			"Resolved location was not a directory!",
		);

		return SataResponse::new(
			pid,
			request_header,
			SataQueryResponse::ErrorCode(FOLDER_REQUIRED_ERROR),
		);
	}

	let Ok(iterator) = read_dir(fs_location.resolved_path()) else {
		debug!(
			packet.typ = "PCFSSrvGetInfo",
			packet.sub_type = "handle_folder_size",
			"Failed to open up iterator over directory!",
		);

		return SataResponse::new(
			pid,
			request_header,
			SataQueryResponse::ErrorCode(PATH_NOT_EXIST_ERROR),
		);
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

			return SataResponse::new(
				pid,
				request_header,
				SataQueryResponse::ErrorCode(SIZE_TOO_BIG_ERROR),
			);
		}

		count += 1;
	}

	SataResponse::new(pid, request_header, SataQueryResponse::SmallSize(count))
}

/// Get information about a particular file on disk.
async fn handle_file_info(
	pid: u32,
	request_header: SataPacketHeader,
	fs: &HostFilesystem,
	location: ResolvedLocation,
) -> SataResponse<SataQueryResponse> {
	match location {
		ResolvedLocation::Filesystem(ref filesystem) => {
			let Ok(metadata) = filesystem.resolved_path().metadata() else {
				debug!(
					packet.location = valuable(&location),
					packet.typ = "PCFSSrvGetInfo",
					packet.sub_type = "handle_file_info",
					"Failed to resolve path!",
				);

				return SataResponse::new(
					pid,
					request_header,
					SataQueryResponse::ErrorCode(PATH_NOT_EXIST_ERROR),
				);
			};
			if &fs.disc_emu_path() == filesystem.resolved_path() {
				return SataResponse::new(
					pid,
					request_header,
					SataQueryResponse::FDInfo(
						// Yes, i know this claims to be an empty file, but this is
						// legitimately how the official software responds.
						SataFDInfo::create_fake_info(0x8000_0000, 0x666, 0, 0, 0),
					),
				);
			}
			let info = SataFDInfo::get_info(fs, &metadata, filesystem.resolved_path()).await;

			debug!(
				packet.location = valuable(&location),
				packet.typ = "PCFSSrvGetInfo",
				packet.sub_type = "handle_file_info",
				packet.result = valuable(&info),
				"Successfully stat'd file!",
			);

			SataResponse::new(pid, request_header, SataQueryResponse::FDInfo(info))
		}
		ResolvedLocation::Network(ref _network) => {
			todo!("Network shares not yet implemented!");
		}
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use crate::fsemul::{
		host_filesystem::test_helpers::{create_temporary_host_filesystem, join_many},
		pcfs::sata::proto::SataCommandInfo,
	};
	use bytes::Bytes;

	#[tokio::test]
	pub async fn disk_size_query_type() {
		// We don't know what your actual disk size here is in tests.
		//
		// BUT, we can confirm it populates the right bytes.
		let (_tempdir, fs) = create_temporary_host_filesystem().await;
		let request = SataGetInfoByQueryPacketBody::new(
			"/%MLC_EMU_DIR/".to_owned(),
			SataQueryType::FreeDiskSpace,
		)
		.expect("Failed to create get info by query packet body");
		let mocked_header = SataPacketHeader::new(0);
		let mocked_ci = SataCommandInfo::new((0, 0), (0, 0), 0);

		let mut space: Bytes = handle_get_info_by_query(
			State(PCFSServerState::new(true, fs, 0)),
			Body(SataRequest::new(mocked_header, mocked_ci, request)),
		)
		.await
		.try_into()
		.expect("Failed to serialize free disk response");
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

	#[tokio::test]
	pub async fn size_of_folder_query_type() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
		let request = SataGetInfoByQueryPacketBody::new(
			"/%MLC_EMU_DIR/my-directory/".to_owned(),
			SataQueryType::SizeOfFolder,
		)
		.expect("Failed to create size of folder request");
		let mocked_header = SataPacketHeader::new(0);
		let mocked_ci = SataCommandInfo::new((0, 0), (0, 0), 0);

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

		let mut size_of_folder: Bytes = handle_get_info_by_query(
			State(PCFSServerState::new(true, fs, 0)),
			Body(SataRequest::new(mocked_header, mocked_ci, request)),
		)
		.await
		.try_into()
		.expect("Failed to serialize size of folder response");
		assert_eq!(
			size_of_folder.len(),
			88 + 0x20,
			"Packet is not correct size!"
		);
		// Okay first chop off the header, we don't care.
		_ = size_of_folder.split_to(0x20);
		// RC + padding bytes.
		assert_eq!(
			[
				size_of_folder[0],
				size_of_folder[1],
				size_of_folder[2],
				size_of_folder[3],
				size_of_folder[4],
				size_of_folder[5],
				size_of_folder[6],
				size_of_folder[7]
			],
			[0_u8; 8],
			"Header was not 8 empty bytes!",
		);
		// Folder size! In our case, 4096 + 8192
		assert_eq!(
			[
				size_of_folder[8],
				size_of_folder[9],
				size_of_folder[10],
				size_of_folder[11]
			],
			[0x00, 0x00, 0x30, 0x00],
			"Calculated folder size was not correct!",
		);
		assert_eq!(
			&size_of_folder[12..],
			&[0; 76],
			"Trailer of folder size must be empty!"
		);
	}

	#[tokio::test]
	pub async fn file_count_query_type() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
		let request = SataGetInfoByQueryPacketBody::new(
			"/%SLC_EMU_DIR/my-directory/".to_owned(),
			SataQueryType::FileCount,
		)
		.expect("Failed to create file count query");
		let mocked_header = SataPacketHeader::new(0);
		let mocked_ci = SataCommandInfo::new((0, 0), (0, 0), 0);

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

		let mut file_count: Bytes = handle_get_info_by_query(
			State(PCFSServerState::new(true, fs, 0)),
			Body(SataRequest::new(mocked_header, mocked_ci, request)),
		)
		.await
		.try_into()
		.expect("Failed to serialize size of folder response");
		assert_eq!(file_count.len(), 88 + 0x20, "Packet is not correct size!");
		// Okay first chop off the header, we don't care.
		_ = file_count.split_to(0x20);
		// RC + padding bytes.
		assert_eq!(
			[file_count[0], file_count[1], file_count[2], file_count[3]],
			[0_u8; 4],
			"Header was not 4 empty bytes!",
		);
		// File count
		assert_eq!(
			[file_count[4], file_count[5], file_count[6], file_count[7]],
			[0x00, 0x00, 0x0, 0x02],
			"Calculated folder size was not correct!",
		);
		assert_eq!(
			&file_count[8..],
			&[0; 80],
			"Trailer of folder size must be empty!"
		);
	}

	#[tokio::test]
	pub async fn file_info_query_type() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
		let request = SataGetInfoByQueryPacketBody::new(
			"/%SLC_EMU_DIR/to-query/file.txt".to_owned(),
			SataQueryType::FileDetails,
		)
		.expect("Failed to create file details packet!");
		let mocked_header = SataPacketHeader::new(0);
		let mocked_ci = SataCommandInfo::new((0, 0), (0, 0), 0);

		// Okay let's create some files and some sizes in there.
		let base_dir = join_many(tempdir.path(), ["data", "slc", "to-query"]);
		tokio::fs::create_dir(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		tokio::fs::write(join_many(&base_dir, ["file.txt"]), vec![0; 1307])
			.await
			.expect("Failed to write test file!");

		let mut file_details: Bytes = handle_get_info_by_query(
			State(PCFSServerState::new(true, fs, 0)),
			Body(SataRequest::new(mocked_header, mocked_ci, request)),
		)
		.await
		.try_into()
		.expect("Failed to serialize file details response!");
		assert_eq!(file_details.len(), 88 + 0x20, "Packet is not correct size!");
		// Okay first chop off the header, we don't care.
		_ = file_details.split_to(0x20);
		assert_eq!(
			&file_details[..40],
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
				file_details[40],
				file_details[41],
				file_details[42],
				file_details[43],
				file_details[44],
				file_details[45],
				file_details[46],
				file_details[47]
			],
			[0_u8; 8],
		);
		assert_ne!(
			[
				file_details[48],
				file_details[49],
				file_details[50],
				file_details[51],
				file_details[52],
				file_details[53],
				file_details[54],
				file_details[55]
			],
			[0_u8; 8],
		);
		assert_eq!(&file_details[56..], &[0; 32]);
	}
}
