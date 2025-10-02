//! Generate a "PADLOG".
//!
//! A PADLOG is built to identify read-file paddings when dealing with a single
//! stream of inputs, like talking to a real cat-dev. For multiple sessions you
//! would need to use something like `SessionManager` to have multiple cat-dev's
//! on multiple unique ports.

use crate::{
	commands::utils::{PacketOnPort, PacketsWithDataOnPort, validate_pcap_path_constraints},
	exit_codes::{
		PADLOG_CANT_CREATE_LOG, PADLOG_FLUSH_FAILURE, PADLOG_NAGLE_FAILURE, PADLOG_WRITE_FAILURE,
	},
};
use bytes::{Buf, Bytes, BytesMut};
use cat_dev::{
	fsemul::pcfs::sata::{
		proto::{
			SataFileDescriptorResult, SataOpenFilePacketBody, SataPingPacketBody,
			SataReadFilePacketBody, SataRequest, SataResponse, SataWriteFilePacketBody,
		},
		server::connection_flags::SataConnectionFlags,
	},
	net::models::{Endianness, NagleGuard},
};
use fnv::FnvHashMap;
use std::{
	cmp::Ordering as CompareOrdering,
	collections::{VecDeque, hash_map::Entry},
	path::Path,
};
use tokio::{
	fs::File,
	io::{AsyncWriteExt, BufWriter, Error as AsyncIOError},
};
use tracing::{Instrument, error, error_span, info};

/// Handle generating a WAL Log from a particular pcap.
pub async fn handle_padlog(pcap_path: &Path, log_path: &Path, sata_port: u16) {
	let final_path = validate_pcap_path_constraints(pcap_path);
	let writer = get_log_writer(log_path).await;
	let iterator = PacketsWithDataOnPort::new(&final_path, sata_port);

	process_packets(iterator, writer)
		.instrument(error_span!(
			"dgswfp::command::padlog::process_packets",
			pcap.path = final_path,
			sata.port = sata_port,
		))
		.await;
}

#[allow(
	// TODO(mythra): fix
	clippy::too_many_lines,
	clippy::type_complexity,
)]
async fn process_packets(packet_stream: PacketsWithDataOnPort, mut writer: BufWriter<File>) {
	let mut stream_buffers: FnvHashMap<
		u64,
		(
			SataConnectionFlags,
			(BytesMut, usize),
			(VecDeque<String>, Option<(i32, BytesMut, usize, usize)>),
		),
	> = FnvHashMap::default();
	let mut fd_map: FnvHashMap<i32, String> = FnvHashMap::default();

	for pkt in packet_stream {
		if let Entry::Vacant(entry) = stream_buffers.entry(pkt.stream_id()) {
			entry.insert((
				// Start as (false, false) as it's the only ping we can't
				// really auto detect.
				//
				// Every other PCFS implementation will start with a ping
				// which if any flags are registered wil be picked up.
				SataConnectionFlags::new_with_flags(false, false),
				(BytesMut::new(), 0_usize),
				(VecDeque::new(), None),
			));
		}

		let (cflags, cache, (cached_file_name, response_buff)) = stream_buffers
			.get_mut(&pkt.stream_id())
			.expect("impossible: always created");
		do_packet_processing(
			pkt,
			&mut fd_map,
			cflags,
			cache,
			cached_file_name,
			response_buff,
			&mut writer,
		)
		.await;
	}

	for (_sid, (_cflags, _cache, (_cached_file_name, mut response_cache))) in stream_buffers {
		if let Some((fd, cached_read_file, expected_size, flags_size)) = response_cache.take() {
			let mut final_rf = cached_read_file.freeze();
			// 32 bytes read file header
			// 4 bytes file len
			let read_file_header = final_rf.split_to(36);

			match final_rf.len().cmp(&expected_size) {
				CompareOrdering::Greater => {
					check_write(
						writer
							.write_all(
								format!(
									"OVER({}): {} [{}]",
									flags_size < expected_size,
									final_rf.len() - expected_size,
									fd_map.get(&fd).cloned().unwrap_or_default(),
								)
								.as_bytes(),
							)
							.await,
					);
				}
				CompareOrdering::Less => {
					check_write(
						writer
							.write_all(
								format!(
									"UNDR({}): {} | {} [{}]",
									flags_size < expected_size,
									expected_size - final_rf.len(),
									if final_rf.len() > flags_size {
										format!("+{}", final_rf.len() - flags_size)
									} else {
										format!("-{}", flags_size - final_rf.len())
									},
									fd_map.get(&fd).cloned().unwrap_or_default(),
								)
								.as_bytes(),
							)
							.await,
					);
				}
				CompareOrdering::Equal => {
					let mut padded_bytes = 0_usize;
					for byte in final_rf.iter().rev() {
						if *byte == 0xCD {
							padded_bytes += 1;
						} else {
							break;
						}
					}

					if padded_bytes == 0 {
						check_write(
							writer
								.write_all(
									format!(
										"UPAD({}): {} [{}]",
										flags_size < expected_size,
										final_rf.len(),
										fd_map.get(&fd).cloned().unwrap_or_default(),
									)
									.as_bytes(),
								)
								.await,
						);
					} else {
						check_write(
							writer
								.write_all(
									format!(
										"PADD({}): {}/{} [{}]",
										flags_size < expected_size,
										final_rf.len() - padded_bytes,
										padded_bytes,
										fd_map.get(&fd).cloned().unwrap_or_default(),
									)
									.as_bytes(),
								)
								.await,
						);
					}
				}
			}

			if read_file_header.starts_with(&[0xC4, 0x00, 0x24, 0x02, 0xE8, 0xEF, 0x24, 0x02]) {
				check_write(writer.write_all(b"  -> C4 Known Header").await);
			} else if read_file_header.starts_with(&[0; 8]) {
				check_write(writer.write_all(b"  -> 00 Known Header").await);
			} else {
				check_write(
					writer
						.write_all(b"  -> ?? Unknown Header: {read_file_header:02x?}")
						.await,
				);
			}
		}
	}

	do_flush(&mut writer).await;
}

#[allow(
	// TODO(mythra): fix
	clippy::too_many_lines,
	clippy::type_complexity,
)]
async fn do_packet_processing(
	pkt: PacketOnPort,
	fd_map: &mut FnvHashMap<i32, String>,
	cflags: &SataConnectionFlags,
	req_nagle_cache: &mut (BytesMut, usize),
	cached_file_names: &mut VecDeque<String>,
	response_cache: &mut Option<(i32, BytesMut, usize, usize)>,
	writer: &mut BufWriter<File>,
) {
	if !pkt.is_request() {
		if let Some(to_extend) = response_cache.as_mut() {
			// Part of a read file response.
			to_extend.1.extend(pkt.data());
		} else if let Some(file_name) = cached_file_names.pop_front() {
			let Ok(response) =
				SataResponse::<SataFileDescriptorResult>::try_from(pkt.data().clone())
			else {
				return;
			};

			if let Ok(fd) = response.body().result() {
				fd_map.insert(fd, file_name);
			}
		}

		return;
	}

	req_nagle_cache.0.extend(pkt.data());
	if let Some((fd, cached_read_file, expected_size, flags_size)) = response_cache.take() {
		let mut final_rf = cached_read_file.freeze();
		// 32 bytes read file header
		// 4 bytes file len
		let read_file_header = final_rf.split_to(36);

		match final_rf.len().cmp(&expected_size) {
			CompareOrdering::Greater => {
				check_write(
					writer
						.write_all(
							format!(
								"OVER({}): {} [{}]",
								flags_size < expected_size,
								final_rf.len() - expected_size,
								fd_map.get(&fd).cloned().unwrap_or_default(),
							)
							.as_bytes(),
						)
						.await,
				);
			}
			CompareOrdering::Less => {
				check_write(
					writer
						.write_all(
							format!(
								"UNDR({}): {} | {} [{}]",
								flags_size < expected_size,
								expected_size - final_rf.len(),
								if final_rf.len() > flags_size {
									format!("+{}", final_rf.len() - flags_size)
								} else {
									format!("-{}", flags_size - final_rf.len())
								},
								fd_map.get(&fd).cloned().unwrap_or_default(),
							)
							.as_bytes(),
						)
						.await,
				);
			}
			CompareOrdering::Equal => {
				let mut padded_bytes = 0_usize;
				for byte in final_rf.iter().rev() {
					if *byte == 0xCD {
						padded_bytes += 1;
					} else {
						break;
					}
				}

				if padded_bytes == 0 {
					check_write(
						writer
							.write_all(
								format!(
									"UPAD({}): {} [{}]",
									flags_size < expected_size,
									final_rf.len(),
									fd_map.get(&fd).cloned().unwrap_or_default(),
								)
								.as_bytes(),
							)
							.await,
					);
				} else {
					check_write(
						writer
							.write_all(
								format!(
									"PADD({}): {}/{} [{}]",
									flags_size < expected_size,
									final_rf.len() - padded_bytes,
									padded_bytes,
									fd_map.get(&fd).cloned().unwrap_or_default(),
								)
								.as_bytes(),
							)
							.await,
					);
				}
			}
		}

		if read_file_header.starts_with(&[0xC4, 0x00, 0x24, 0x02, 0xE8, 0xEF, 0x24, 0x02]) {
			check_write(writer.write_all(b"  -> C4 Known Header").await);
		} else if read_file_header.starts_with(&[0; 8]) {
			check_write(writer.write_all(b"  -> 00 Known Header").await);
		} else {
			check_write(
				writer
					.write_all(b"  -> ?? Unknown Header: {read_file_header:02x?}")
					.await,
			);
		}
	}

	let nagle_guard = NagleGuard::U32LengthPrefixed(Endianness::Big, Some(0x20));
	'state_loop: loop {
		if req_nagle_cache.1 > 0 {
			if req_nagle_cache.0.len() < req_nagle_cache.1 {
				break;
			}

			(&mut req_nagle_cache.0).take(req_nagle_cache.1);
			req_nagle_cache.1 = 0;
		} else {
			while let Ok(Some((start_of_packet, end_of_packet))) =
				nagle_guard.split(&req_nagle_cache.0)
			{
				let remaining_buff = req_nagle_cache.0.split_off(end_of_packet);
				let _start_of_buff = req_nagle_cache.0.split_to(start_of_packet);
				let body = req_nagle_cache.0.clone().freeze();
				req_nagle_cache.0 = remaining_buff;

				if let Some(skip_amount) =
					process_request(cflags, cached_file_names, response_cache, &body)
				{
					req_nagle_cache.1 = skip_amount;
					continue 'state_loop;
				}
			}
		}
		break;
	}
}

#[allow(
	// TODO(mythra): fix
	clippy::too_many_lines,
)]
fn process_request(
	cflags: &SataConnectionFlags,
	cached_file_names: &mut VecDeque<String>,
	response_cache: &mut Option<(i32, BytesMut, usize, usize)>,
	packet: &Bytes,
) -> Option<usize> {
	// Can't sniff a command type out of this...
	if packet.len() < 0x34 {
		return None;
	}

	let command_type = u32::from_be_bytes([packet[0x30], packet[0x31], packet[0x32], packet[0x33]]);
	if command_type == 5 {
		let parsed_data = match SataRequest::<SataOpenFilePacketBody>::try_from(packet.clone()) {
			Ok(d) => d,
			Err(cause) => {
				error!(
					?cause,
					id = "dgswfp::generate::open_file_parse_failure",
					packet = format!("{packet:02x?}"),
					"Failed to parse open file request, so cannot determine correct nagle length!",
				);

				std::process::exit(PADLOG_NAGLE_FAILURE);
			}
		};

		cached_file_names.push_back(parsed_data.body().path().to_owned());
	} else if command_type == 6 {
		// We want to determine the padding of this.
		assert!(
			!response_cache.is_some(),
			"TODO(mythra): double response cache"
		);
		let parsed_data = match SataRequest::<SataReadFilePacketBody>::try_from(packet.clone()) {
			Ok(d) => d,
			Err(cause) => {
				error!(
					?cause,
					id = "dgswfp::generate::read_file_parse_failure",
					packet = format!("{packet:02x?}"),
					"Failed to parse read file request, so cannot determine correct nagle length!",
				);

				std::process::exit(PADLOG_NAGLE_FAILURE);
			}
		};

		let total_size = parsed_data.body().block_size() * parsed_data.body().block_count();

		_ = response_cache.insert((
			parsed_data.body().file_descriptor(),
			BytesMut::new(),
			usize::try_from(total_size).unwrap_or(usize::MAX),
			usize::try_from(cflags.first_read_size()).unwrap_or(usize::MAX),
		));
	} else if command_type == 7 {
		// This is a write file which will break our normal nagle logic.
		// Pre-calculate how many bytes we'll send and mark it.
		let parsed_data = match SataRequest::<SataWriteFilePacketBody>::try_from(packet.clone()) {
			Ok(d) => d,
			Err(cause) => {
				error!(
					?cause,
					id = "dgswfp::generate::write_file_parse_failure",
					packet = format!("{packet:02x?}"),
					"Failed to parse write file request, so cannot determine correct nagle length!",
				);

				std::process::exit(PADLOG_NAGLE_FAILURE);
			}
		};

		return Some(
			usize::try_from(parsed_data.body().block_size()).unwrap_or(usize::MAX)
				* usize::try_from(parsed_data.body().block_count()).unwrap_or(usize::MAX),
		);
	} else if command_type == 0x14 {
		// This is a ping record first read file size / write file size
		let parsed_data = match SataRequest::<SataPingPacketBody>::try_from(packet.clone()) {
			Ok(d) => d,
			Err(cause) => {
				error!(
					?cause,
					id = "dgswfp::generate::ping_parse_failure",
					packet = format!("{packet:02x?}"),
					"Failed to parse ping request, so cannot determine correct nagle length!",
				);

				std::process::exit(PADLOG_NAGLE_FAILURE);
			}
		};

		info!(
			id = "dgswfp::padlog::update_read_write_size",
			read_size = parsed_data.command_info().user().0,
			write_size = parsed_data.command_info().user().1,
			"Updating Connection Read/Write Size",
		);
		cflags.set_first_read_size(parsed_data.command_info().user().0);
		cflags.set_first_write_size(parsed_data.command_info().user().1);
	}

	None
}

/// Create a buffered writer to a particular log file.
async fn get_log_writer(log_path: &Path) -> BufWriter<File> {
	let file = match File::create_new(log_path).await {
		Ok(fd) => fd,
		Err(cause) => {
			error!(
				?cause,
				id = "dgswfp::padlog::cannot_open_log",
				log.path = %log_path.display(),
				help = "Please ensure the LOG location you specified is correct, and does not exist.",
				"Could not create destination log file!",
			);

			std::process::exit(PADLOG_CANT_CREATE_LOG);
		}
	};

	BufWriter::new(file)
}

async fn do_flush(writer: &mut BufWriter<File>) {
	if let Err(cause) = writer.flush().await {
		error!(
			?cause,
			id = "dgswfp::padlog::cannot_flush_log_file",
			"Could not write all data to our log file, may be corrupt!",
		);

		std::process::exit(PADLOG_FLUSH_FAILURE);
	}
}

fn check_write(result: Result<(), AsyncIOError>) {
	if let Err(cause) = result {
		error!(
			?cause,
			id = "dgswfp::padlog::cannot_write_to_buffer",
			"Could not write data to our in memory buffer to later flush to a file, OOM?",
		);

		std::process::exit(PADLOG_WRITE_FAILURE);
	}
}
