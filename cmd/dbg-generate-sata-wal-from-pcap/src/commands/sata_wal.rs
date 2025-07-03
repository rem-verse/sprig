//! Handles generation of a SATA WAL log from a PCAPNG.

use crate::{
	SHOULD_LOG_JSON,
	commands::utils::{PacketOnPort, PacketsWithDataOnPort, validate_pcap_path_constraints},
	exit_codes::{GENERATE_CANT_CREATE_WAL, GENERATE_NAGLE_FAILURE},
	utils::add_context_to,
};
use bytes::{Bytes, BytesMut};
use cat_dev::{
	fsemul::pcfs::sata::{
		proto::{SataPingPacketBody, SataReadFilePacketBody, SataRequest, SataWriteFilePacketBody},
		server::{connection_flags::SataConnectionFlags, wal::WriteAheadLog},
	},
	net::models::{Endianness, NagleGuard},
};
use fnv::FnvHashMap;
use miette::miette;
use std::{
	collections::hash_map::Entry,
	path::{Path, PathBuf},
	time::Duration,
};
use tokio::time::sleep;
use tracing::{Instrument, debug, error, error_span, info};

/// Handle generating a WAL Log from a particular pcap.
pub async fn handle_generate(pcap_path: PathBuf, wal_path: PathBuf, sata_port: u16) {
	let final_path = validate_pcap_path_constraints(&pcap_path);
	let stream = PacketsWithDataOnPort::new(&final_path, sata_port);
	let wal = get_wal(&wal_path);

	process_packets(stream, wal)
		.instrument(error_span!(
			"dgswfp::command::sata_wal::process_packets",
			pcap.path = final_path,
			sata.port = sata_port,
		))
		.await;
}

#[allow(
	// TODO(mythra): fix
	clippy::type_complexity,
	clippy::too_many_lines,
)]
async fn process_packets(packet_stream: PacketsWithDataOnPort, wal: WriteAheadLog) {
	let mut stream_buffers: FnvHashMap<
		u64,
		(
			SataConnectionFlags,
			(SataStreamState, BytesMut),
			(SataStreamState, BytesMut),
		),
	> = FnvHashMap::default();

	for pkt in packet_stream {
		if let Entry::Vacant(entry) = stream_buffers.entry(pkt.stream_id()) {
			wal.record_open_stream(pkt.stream_id()).await;
			entry.insert((
				// Start as (false, false) as it's the only ping we can't
				// really auto detect.
				//
				// Every other PCFS implementation will start with a ping
				// which if any flags are registered wil be picked up.
				SataConnectionFlags::new_with_flags(false, false),
				(SataStreamState::ProcessingPackets, BytesMut::new()),
				(SataStreamState::ProcessingPackets, BytesMut::new()),
			));
		}

		let ref_buffers = stream_buffers
			.get_mut(&pkt.stream_id())
			.expect("impossible: always created");
		let (flags, cache, state, other_state) = if pkt.is_request() {
			let mut_borrow: &mut (
				SataConnectionFlags,
				(SataStreamState, BytesMut),
				(SataStreamState, BytesMut),
			) = ref_buffers;

			(
				&mut_borrow.0,
				&mut mut_borrow.1.1,
				&mut mut_borrow.1.0,
				&mut mut_borrow.2.0,
			)
		} else {
			let mut_borrow: &mut (
				SataConnectionFlags,
				(SataStreamState, BytesMut),
				(SataStreamState, BytesMut),
			) = ref_buffers;

			(
				&mut_borrow.0,
				&mut mut_borrow.2.1,
				&mut mut_borrow.2.0,
				&mut mut_borrow.1.0,
			)
		};

		do_packet_processing(pkt, cache, flags, state, other_state, &wal).await;
	}

	info!(
		id = "dgswfp::generate::flush_notice",
		"Packets processed... flushing WAL"
	);
	// Ensure flush happens by ensuring enough seconds have passed that a close
	// stream will trigger a flush.
	sleep(Duration::from_secs(3)).await;
	for sid in stream_buffers.keys() {
		wal.record_close_stream(*sid).await;
	}
}

async fn do_packet_processing(
	pkt: PacketOnPort,
	nagle_cache: &mut BytesMut,
	cflags: &SataConnectionFlags,
	state: &mut SataStreamState,
	other_state: &mut SataStreamState,
	wal: &WriteAheadLog,
) {
	// Always add ourselves to whatever cache we already have.
	// We'll split below...
	nagle_cache.extend(pkt.data());

	let nagle_guard = NagleGuard::U32LengthPrefixed(Endianness::Big, Some(0x20));
	'stream_state_loop: loop {
		if let SataStreamState::NeedsAtLeast(fd, needed) = state {
			if nagle_cache.len() < *needed {
				// Sniff out read file errors...
				if !pkt.is_request() && nagle_cache.starts_with(&[0xC4_u8, 0x00, 0xFE, 0x00, 0x20])
				{
					*needed = 8 + 24 + 4;
				}

				if nagle_cache.len() < *needed {
					debug!("waiting for more data in NeedsAtLeast....");
					return;
				}
			}
			let next_item = nagle_cache.split_to(*needed).freeze();

			if pkt.is_request() {
				wal.record_oob_file_write_read(pkt.stream_id(), *fd, *needed)
					.await;
			} else {
				wal.record_response(pkt.stream_id(), next_item).await;
			}
			*state = SataStreamState::ProcessingPackets;
			continue;
		}
		if let SataStreamState::NeedsAtLeastCheckAt(fd, needed_at) = state {
			// Sniff out read file errors...
			if !pkt.is_request() && nagle_cache.starts_with(&[0xC4_u8, 0x00, 0xFE, 0x00, 0x20]) {
				let needed = 8 + 24 + 4;
				if nagle_cache.len() < needed {
					debug!("waiting for more data in error'd NeedsAtLeastCheckAt");
					return;
				}
			}

			if nagle_cache.len() < *needed_at + 4 {
				debug!("waiting for more data for length check in NeedsAtLeastCheckAt....");
				return;
			}
			let read_bytes_or_file_size = u32::from_be_bytes([
				nagle_cache[*needed_at],
				nagle_cache[*needed_at + 1],
				nagle_cache[*needed_at + 2],
				nagle_cache[*needed_at + 3],
			]);
			let read_bytes_or_file_size_size =
				usize::try_from(read_bytes_or_file_size).unwrap_or(usize::MAX);

			// Okay, so we've got a file length, or read length. So we now need to
			// check. Are we going to do padding? We know to hit this branch...
			// you _MUST_ have total_read_size > read_file.len(). So now, we need
			// to check the _other_ condition for padding. Is
			// file_size < read_file.len().
			if read_bytes_or_file_size < cflags.first_read_size() {
				// Okay so we're large enough that we got into this state, but we're not
				// large enough to ignore padding. So switch our state, and pad.
				if pkt.is_request() {
					unreachable!();
				} else {
					*state = SataStreamState::NeedsAtLeast(
						*fd,
						0x20 + 0x4
							+ usize::try_from(cflags.first_read_size()).unwrap_or(usize::MAX),
					);
				}

				continue;
			}

			if nagle_cache.len() < *needed_at + 4 + read_bytes_or_file_size_size {
				debug!("waiting for more data in NeedsAtLeastCheckAt....");
				return;
			}

			let next_item = nagle_cache
				.split_to(*needed_at + 4 + read_bytes_or_file_size_size)
				.freeze();
			if pkt.is_request() {
				// This branch is only taken in read file, not write file
				unreachable!();
			} else {
				wal.record_response(pkt.stream_id(), next_item).await;
			}
			*state = SataStreamState::ProcessingPackets;
			continue;
		}

		while let Ok(Some((start_of_packet, end_of_packet))) = nagle_guard.split(nagle_cache) {
			let remaining_buff = nagle_cache.split_off(end_of_packet);
			let _start_of_buff = nagle_cache.split_to(start_of_packet);
			let body = nagle_cache.clone().freeze();
			*nagle_cache = remaining_buff;

			let re_loop = if pkt.is_request() {
				process_request(pkt.stream_id(), body, cflags, state, other_state, wal).await
			} else {
				process_response(pkt.stream_id(), body, cflags, state, other_state, wal).await
			};

			if re_loop {
				continue 'stream_state_loop;
			}
		}
		break;
	}
}

#[allow(
	// TODO(mythra): fix
	clippy::too_many_lines,
)]
async fn process_request(
	stream_id: u64,
	packet: Bytes,
	cflags: &SataConnectionFlags,
	state: &mut SataStreamState,
	response_state: &mut SataStreamState,
	wal: &WriteAheadLog,
) -> bool {
	wal.record_request(stream_id, packet.clone()).await;
	// Can't sniff a command type out of this...
	if packet.len() < 0x34 {
		return false;
	}

	let command_type = u32::from_be_bytes([packet[0x30], packet[0x31], packet[0x32], packet[0x33]]);
	if command_type == 6 && cflags.ffio_enabled() {
		// This is a read file, which will break normal nagle logic.
		// Let's pre-calculate the size the server should send, and
		// mark it.
		let parsed_data = match SataRequest::<SataReadFilePacketBody>::try_from(packet.clone()) {
			Ok(d) => d,
			Err(cause) => {
				if SHOULD_LOG_JSON() {
					error!(
						?cause,
						id = "dgswfp::generate::read_file_parse_failure",
						packet = format!("{packet:02x?}"),
						"Failed to parse read file request, so cannot determine correct nagle length!",
					);
				} else {
					error!(
						"\n{:?}",
						add_context_to(
							miette!(
								"Failed to parse read file request, so cannot determine correct nagle length!"
							),
							[cause.into()].into_iter(),
						),
					);
				}

				std::process::exit(GENERATE_NAGLE_FAILURE);
			}
		};

		let total_size = parsed_data.body().block_size() * parsed_data.body().block_count();
		if total_size > cflags.first_read_size() {
			*response_state = SataStreamState::NeedsAtLeastCheckAt(
				parsed_data.body().file_descriptor(),
				0x20_usize,
			);
		} else {
			*response_state = SataStreamState::NeedsAtLeast(
				parsed_data.body().file_descriptor(),
				0x20_usize
					+ 0x4_usize + (usize::try_from(parsed_data.body().block_size())
					.unwrap_or(usize::MAX)
					* usize::try_from(parsed_data.body().block_count()).unwrap_or(usize::MAX)),
			);
		}
	} else if command_type == 7 {
		// This is a write file which will break our normal nagle logic.
		// Pre-calculate how many bytes we'll send and mark it.
		let parsed_data = match SataRequest::<SataWriteFilePacketBody>::try_from(packet.clone()) {
			Ok(d) => d,
			Err(cause) => {
				if SHOULD_LOG_JSON() {
					error!(
						?cause,
						id = "dgswfp::generate::write_file_parse_failure",
						packet = format!("{packet:02x?}"),
						"Failed to parse write file request, so cannot determine correct nagle length!",
					);
				} else {
					error!(
						"\n{:?}",
						add_context_to(
							miette!(
								"Failed to parse write file request, so cannot determine correct nagle length!"
							),
							[cause.into()].into_iter(),
						),
					);
				}

				std::process::exit(GENERATE_NAGLE_FAILURE);
			}
		};

		*state = SataStreamState::NeedsAtLeast(
			parsed_data.body().file_descriptor(),
			usize::try_from(parsed_data.body().block_size()).unwrap_or(usize::MAX)
				* usize::try_from(parsed_data.body().block_count()).unwrap_or(usize::MAX),
		);
		// We modified our state, we need to loop!
		return true;
	} else if command_type == 0x14 {
		// This is a ping record first read file size / write file size
		let parsed_data = match SataRequest::<SataPingPacketBody>::try_from(packet.clone()) {
			Ok(d) => d,
			Err(cause) => {
				if SHOULD_LOG_JSON() {
					error!(
						?cause,
						id = "dgswfp::generate::ping_parse_failure",
						packet = format!("{packet:02x?}"),
						"Failed to parse ping request, so cannot determine correct nagle length!",
					);
				} else {
					error!(
						"\n{:?}",
						add_context_to(
							miette!(
								"Failed to parse ping request, so cannot determine correct nagle length!"
							),
							[cause.into()].into_iter(),
						),
					);
				}

				std::process::exit(GENERATE_NAGLE_FAILURE);
			}
		};

		info!(
			read_size = parsed_data.command_info().user().0,
			write_size = parsed_data.command_info().user().1,
			"Updating Connection Read/Write Size",
		);
		cflags.set_first_read_size(parsed_data.command_info().user().0);
		cflags.set_first_write_size(parsed_data.command_info().user().1);
	}

	false
}

async fn process_response(
	stream_id: u64,
	packet: Bytes,
	cflags: &SataConnectionFlags,
	_state: &mut SataStreamState,
	_request_state: &mut SataStreamState,
	wal: &WriteAheadLog,
) -> bool {
	// Try to sniff out pings...
	if packet.len() == 0x28 {
		if packet.ends_with(&0xCAFE_0003_u32.to_be_bytes()) {
			cflags.set_csr_enabled(true);
			cflags.set_ffio_enabled(true);
		} else if packet.ends_with(&0xCAFE_0002_u32.to_be_bytes()) {
			cflags.set_csr_enabled(true);
			cflags.set_ffio_enabled(false);
		} else if packet.ends_with(&0xCAFE_0001_u32.to_be_bytes()) {
			cflags.set_csr_enabled(false);
			cflags.set_ffio_enabled(true);
		}
	}

	wal.record_response(stream_id, packet.clone()).await;

	false
}

/// Get a reference to a live write ahead log.
fn get_wal(wal_path: &Path) -> WriteAheadLog {
	match WriteAheadLog::new(wal_path.to_path_buf()) {
		Ok(wal) => wal,
		Err(cause) => {
			if SHOULD_LOG_JSON() {
				error!(
					?cause,
					id = "dgswfp::generate::cannot_open_wal",
					wal.path = %wal_path.display(),
					"Failed to create a WAL to write!",
				);
			} else {
				error!(
					"\n{:?}",
					add_context_to(
						miette!("cannot open and generate wal"),
						[
							cause.into(),
							miette!(
								"The wal path we would've written to is: {}",
								wal_path.display(),
							),
						]
						.into_iter(),
					),
				);
			}

			std::process::exit(GENERATE_CANT_CREATE_WAL);
		}
	}
}

enum SataStreamState {
	/// The 'default' state, just reading packets.
	ProcessingPackets,
	/// We requested a file read, and need to break NAGLE logic
	/// to read a file of N bytes for our response.
	///
	/// IF this is a request side of the stream in this state it means
	/// we are in write file, and should read N extra bytes and call register oob
	/// write extra read.
	///
	/// IF this is a response side of the stream, it means we need to read N bytes
	/// rather than the normal NAGLE length, and call that as one 'response' packet.
	NeedsAtLeast(i32, usize),
	/// We requested a file read, but one that allows for truncating a response rather
	/// than padding it out.
	///
	/// In this case we need to actually read the repsonse to know how many bytes to
	/// read. it'll be at usize location in the packet.
	NeedsAtLeastCheckAt(i32, usize),
}
