//! Handle opening a file over PCFS.

use crate::{
	fsemul::{
		host_filesystem::{FilesystemLocation, ResolvedLocation},
		pcfs::sata::{
			proto::{
				SataCommandInfo, SataFileDescriptorResult, SataOpenFilePacketBody,
				SataPacketHeader, SataRequest, SataResponse,
			},
			server::PCFSServerState,
		},
	},
	net::{
		additions::StreamID,
		server::requestable::{Body, State},
	},
};
use tokio::fs::{OpenOptions, set_permissions};
use tracing::{debug, warn};

/// A filesystem error occured.
const FS_ERROR: u32 = 0xFFF0_FFE0;
/// An error code to send when a path does not exist.
///
/// This is also used in some places that are a bit of a stretch like for
/// network shares on disk space. The path doesn't exist on a disk, so this
/// error code is used, even if it's not quite exact.
const PATH_NOT_EXIST_ERROR: u32 = 0xFFF0_FFE9;

/// Handle opening a file upon request.
pub async fn handle_open_file(
	stream: StreamID,
	State(state): State<PCFSServerState>,
	Body(request): Body<SataRequest<SataOpenFilePacketBody>>,
) -> SataResponse<SataFileDescriptorResult> {
	let packet = request.body();
	let command_info = request.command_info();
	let request_header = request.header().clone();

	let Ok(final_location) = state.host_filesystem().resolve_path(packet.path()) else {
		debug!(
			packet.path = packet.path(),
			packet.typ = "PCFSSrvOpenFile",
			"Failed to resolve path!",
		);

		return SataResponse::new(
			state.pid(),
			request_header,
			SataFileDescriptorResult::error(PATH_NOT_EXIST_ERROR),
		);
	};
	let ResolvedLocation::Filesystem(fs_location) = final_location else {
		todo!("network shares not yet implemented!")
	};

	// Since you can only have one of 'raw', if we want to check for
	// 'a' || 'w', we can instead check for the absence of 'r'.
	let mode = packet.mode();
	if let Some(error_response) = update_read_only_flags(
		&request_header,
		command_info,
		&state,
		mode,
		&fs_location,
		packet.path(),
	)
	.await
	{
		return error_response;
	}

	// Okay time to open!
	let mut options = OpenOptions::new();
	if mode.contains('r') {
		options.read(true);
	}
	if mode.contains('w') {
		options.write(true).truncate(true).create(true);
	}
	if mode.contains('a') {
		options.write(true).truncate(false).create(true);
	}
	if mode.contains('+') {
		options.create(true);
	}

	let fd = match state
		.host_filesystem()
		.open_file(options, fs_location.resolved_path(), Some(stream.to_raw()))
		.await
	{
		Ok(fd) => fd,
		Err(cause) => {
			warn!(
				?cause,
				packet.path = packet.path(),
				packet.typ = "PCFSSrvOpenFile",
				"Failed to open file!",
			);

			return SataResponse::new(
				state.pid(),
				request_header,
				SataFileDescriptorResult::error(FS_ERROR),
			);
		}
	};

	debug!(
		result.fd = fd,
		packet.path = packet.path(),
		packet.typ = "PCFSSrvOpenFile",
		"Successfully opened file!",
	);
	SataResponse::new(
		state.pid(),
		request_header,
		SataFileDescriptorResult::success(fd),
	)
}

#[allow(
	// Yes clippy, this is what I want, which is why i wrote it.
	clippy::permissions_set_readonly_false,
)]
async fn update_read_only_flags(
	request_header: &SataPacketHeader,
	command_info: &SataCommandInfo,
	state: &PCFSServerState,
	mode: &str,
	fs_location: &FilesystemLocation,
	path: &str,
) -> Option<SataResponse<SataFileDescriptorResult>> {
	if !state
		.host_filesystem()
		.path_allows_writes(fs_location.resolved_path())
		&& (!mode.contains('r') || mode.contains('+'))
	{
		debug!(
			packet.mode = mode,
			packet.path = path,
			packet.typ = "PCFSSrvOpenFile",
			"Path does not allow opening as writable!",
		);

		return Some(SataResponse::new(
			state.pid(),
			request_header.clone(),
			SataFileDescriptorResult::error(FS_ERROR),
		));
	}
	let [allow_becoming_write, _, _, _] = command_info.capabilities().1.to_be_bytes();
	// If it exists, we potentially need to change read only mode flag.
	if fs_location.resolved_path().exists() {
		let Ok(metadata) = fs_location.resolved_path().metadata() else {
			debug!(
				packet.path = path,
				packet.typ = "PCFSSrvOpenFile",
				"Failed to get metadata of resolved path!",
			);

			return Some(SataResponse::new(
				state.pid(),
				request_header.clone(),
				SataFileDescriptorResult::error(FS_ERROR),
			));
		};
		let mut perms = metadata.permissions();
		if perms.readonly() && !mode.contains('r') && allow_becoming_write == 0 {
			debug!(
				path.is_read_only = perms.readonly(),
				packet.mode = mode,
				packet.path = path,
				packet.typ = "PCFSSrvOpenFile",
				"Path is marked read-only, and mode was requested as non-read!",
			);

			return Some(SataResponse::new(
				state.pid(),
				request_header.clone(),
				SataFileDescriptorResult::error(FS_ERROR),
			));
		}
		perms.set_readonly(false);
		if set_permissions(fs_location.resolved_path(), perms)
			.await
			.is_err()
		{
			debug!(
				packet.path = path,
				packet.typ = "PCFSSrvOpenFile",
				"Failed to update permissions as requested on open!"
			);

			return Some(SataResponse::new(
				state.pid(),
				request_header.clone(),
				SataFileDescriptorResult::error(FS_ERROR),
			));
		}
	}

	None
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use crate::fsemul::host_filesystem::test_helpers::{
		create_temporary_host_filesystem, join_many,
	};
	use bytes::Bytes;

	#[tokio::test]
	pub async fn simple_open_file_request() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
		let request = SataOpenFilePacketBody::new(
			"/%SLC_EMU_DIR/to-query/file.txt".to_owned(),
			"r".to_owned(),
		)
		.expect("Failed to create open file packet body!");
		let mocked_header = SataPacketHeader::new(0);
		let mocked_command_info = SataCommandInfo::new((0, 0), (0, 0), 0x5);

		let base_dir = join_many(tempdir.path(), ["data", "slc", "to-query"]);
		tokio::fs::create_dir(&base_dir)
			.await
			.expect("Failed to create temporary directory for test!");
		tokio::fs::write(join_many(&base_dir, ["file.txt"]), vec![0; 1307])
			.await
			.expect("Failed to write test file!");

		let mut response: Bytes = handle_open_file(
			StreamID::from_existing(1),
			State(PCFSServerState::new(true, fs.clone(), 0)),
			Body(SataRequest::new(
				mocked_header,
				mocked_command_info,
				request,
			)),
		)
		.await
		.try_into()
		.expect("Failed to serialize open file response!");
		assert_eq!(response.len(), 8 + 0x20, "Packet is not correct size!");
		// Okay first chop off the header, we don't care.
		_ = response.split_to(0x20);
		assert_eq!(
			&response[..4],
			&[0x00, 0x00, 0x00, 0x00], // RC
		);
		assert_ne!(
			&response[4..],
			&[0x00, 0x00, 0x00, 0x00], // File handle.
		);
		fs.close_file(
			i32::from_be_bytes([response[4], response[5], response[6], response[7]]),
			Some(1),
		)
		.await;
	}
}
