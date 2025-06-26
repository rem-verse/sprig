//! Generate a "PADLOG".
//!
//! A PADLOG is a way to identify how read-files are handled when sending to
//! a real cat-dev. It is a hacky-ish script that will error out if not given
//! a PCAP that talks like a real cat-dev talks.

use crate::{
	SHOULD_LOG_JSON,
	exit_codes::{
		GENERATE_CANT_OPEN_PCAP, GENERATE_CANT_SPAWN_TSHARK, GENERATE_NAGLE_FAILURE,
		GENERATE_PCAP_DOES_NOT_EXIST,
	},
	utils::add_context_to,
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
use miette::miette;
use rtshark::{RTShark, RTSharkBuilder};
use std::{
	collections::{VecDeque, hash_map::Entry},
	path::Path,
};
use tracing::{debug, error, info};

// TODO(mythra): this is all pretty ugly and hacky, let's find a way to make this better?

/// Handle generating a WAL Log from a particular pcap.
pub fn handle_padlog(pcap_path: &Path, sata_port: u32) {
	let final_path = validate_exists(pcap_path);

	let stream = match RTSharkBuilder::builder()
		.input_path(&final_path)
		.disable_protocol("ALL")
		.enable_protocol("eth")
		.enable_protocol("ip")
		.enable_protocol("tcp")
		.display_filter(&format!("tcp.port == {sata_port} && data.len > 0"))
		.spawn()
	{
		Ok(stream) => stream,
		Err(cause) => {
			if SHOULD_LOG_JSON() {
				error!(
					?cause,
					id = "padlog::generate::cannot_spawn_tshark",
					pcap.path = %pcap_path.display(),
					"cannot spawn tshark",
				);
			} else {
				error!(
					"\n{:?}",
					add_context_to(
						miette!("Failed to spawn TSHARK cli"),
						[
							miette!("{cause:?}"),
							miette!("PCAP Path: {}", pcap_path.display()),
						]
						.into_iter(),
					),
				);
			}

			std::process::exit(GENERATE_CANT_SPAWN_TSHARK);
		}
	};

	process_packets(sata_port, stream);
}

#[allow(
	// TODO(mythra): fix this later.
	clippy::type_complexity,
	clippy::too_many_lines,
	clippy::comparison_chain,
)]
fn process_packets(sata_port: u32, mut packet_stream: RTShark) {
	let mut stream_buffers: FnvHashMap<
		u64,
		(
			SataConnectionFlags,
			(BytesMut, usize),
			(VecDeque<String>, Option<(i32, BytesMut, usize, usize)>),
		),
	> = FnvHashMap::default();
	let mut fd_map: FnvHashMap<i32, String> = FnvHashMap::default();

	while let Ok(Some(pkt)) = packet_stream.read() {
		let Some(tcp_layer) = pkt.layer_name("tcp") else {
			debug!("packet is missing TCP layer! skipping...");
			continue;
		};

		let Some(srcport) = tcp_layer
			.metadata("tcp.srcport")
			.and_then(|val| val.value().parse::<u32>().ok())
		else {
			debug!("packet is missing `tcp.srcport`");
			continue;
		};
		let Some(dstport) = tcp_layer
			.metadata("tcp.dstport")
			.and_then(|val| val.value().parse::<u32>().ok())
		else {
			debug!("packet is missing `tcp.dstport`");
			continue;
		};
		if srcport != sata_port && dstport != sata_port {
			debug!("packet is not for sata port??");
			continue;
		}
		let is_request = dstport == sata_port;

		let Some(sid) = tcp_layer
			.metadata("tcp.stream")
			.and_then(|val| val.value().parse::<u64>().ok())
		else {
			debug!("packet is missing `tcp.stream`");
			continue;
		};
		let Some(payload) = tcp_layer
			.metadata("tcp.payload")
			.map(|val| Bytes::from(string_to_hex_bytes(val.raw_value())))
		else {
			debug!("packet is missing `tcp.payload`");
			continue;
		};

		if let Entry::Vacant(entry) = stream_buffers.entry(sid) {
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

		//SataConnectionFlags, BytesMut, Option<BytesMut>
		let (cflags, cache, (cached_file_name, response_buff)) = stream_buffers
			.get_mut(&sid)
			.expect("impossible: always created");
		do_packet_processing(
			is_request,
			&mut fd_map,
			cflags,
			cache,
			cached_file_name,
			response_buff,
			payload,
		);
	}

	for (_sid, (_cflags, _cache, (_cached_file_name, mut response_cache))) in stream_buffers {
		if let Some((fd, cached_read_file, expected_size, flags_size)) = response_cache.take() {
			let mut final_rf = cached_read_file.freeze();
			// 32 bytes read file header
			// 4 bytes file len
			let read_file_header = final_rf.split_to(36);

			if final_rf.len() > expected_size {
				eprintln!(
					"OVER({}): {} [{}]",
					flags_size < expected_size,
					final_rf.len() - expected_size,
					fd_map.get(&fd).cloned().unwrap_or_default(),
				);
			} else if final_rf.len() < expected_size {
				eprintln!(
					"UNDR({}): {} | {} [{}]",
					flags_size < expected_size,
					expected_size - final_rf.len(),
					if final_rf.len() > flags_size {
						format!("+{}", final_rf.len() - flags_size)
					} else {
						format!("-{}", flags_size - final_rf.len())
					},
					fd_map.get(&fd).cloned().unwrap_or_default(),
				);
			} else {
				let mut padded_bytes = 0_usize;
				for byte in final_rf.iter().rev() {
					if *byte == 0xCD {
						padded_bytes += 1;
					} else {
						break;
					}
				}

				if padded_bytes == 0 {
					eprintln!(
						"UPAD({}): {} [{}]",
						flags_size < expected_size,
						final_rf.len(),
						fd_map.get(&fd).cloned().unwrap_or_default(),
					);
				} else {
					eprintln!(
						"PADD({}): {}/{} [{}]",
						flags_size < expected_size,
						final_rf.len() - padded_bytes,
						padded_bytes,
						fd_map.get(&fd).cloned().unwrap_or_default(),
					);
				}
			}

			if read_file_header.starts_with(&[0xC4, 0x00, 0x24, 0x02, 0xE8, 0xEF, 0x24, 0x02]) {
				eprintln!("  -> header type: C4....");
			} else {
				eprintln!("  -> header type: {}", read_file_header[0]);
			}
		}
	}
}

#[allow(
	// TODO(mythra): fix this later... 
	clippy::too_many_arguments,
	clippy::comparison_chain,
)]
fn do_packet_processing(
	is_request: bool,
	fd_map: &mut FnvHashMap<i32, String>,
	cflags: &SataConnectionFlags,
	req_nagle_cache: &mut (BytesMut, usize),
	cached_file_names: &mut VecDeque<String>,
	response_cache: &mut Option<(i32, BytesMut, usize, usize)>,
	packet: Bytes,
) {
	if !is_request {
		if let Some(to_extend) = response_cache.as_mut() {
			// Part of a read file response.
			to_extend.1.extend(packet);
		} else if let Some(file_name) = cached_file_names.pop_front() {
			let Ok(response) = SataResponse::<SataFileDescriptorResult>::try_from(packet.clone())
			else {
				return;
			};

			if let Ok(fd) = response.body().result() {
				fd_map.insert(fd, file_name);
			}
		}

		return;
	}

	req_nagle_cache.0.extend(packet);
	if let Some((fd, cached_read_file, expected_size, flags_size)) = response_cache.take() {
		let mut final_rf = cached_read_file.freeze();
		// 32 bytes read file header
		// 4 bytes file len
		let read_file_header = final_rf.split_to(36);

		if final_rf.len() > expected_size {
			eprintln!(
				"OVER({}): {} [{}]",
				flags_size < expected_size,
				final_rf.len() - expected_size,
				fd_map.get(&fd).cloned().unwrap_or_default(),
			);
		} else if final_rf.len() < expected_size {
			eprintln!(
				"UNDR({}): {} | {} [{}]",
				flags_size < expected_size,
				expected_size - final_rf.len(),
				if final_rf.len() > flags_size {
					format!("+{}", final_rf.len() - flags_size)
				} else {
					format!("-{}", flags_size - final_rf.len())
				},
				fd_map.get(&fd).cloned().unwrap_or_default(),
			);
		} else {
			let mut padded_bytes = 0_usize;
			for byte in final_rf.iter().rev() {
				if *byte == 0xCD {
					padded_bytes += 1;
				} else {
					break;
				}
			}

			if padded_bytes == 0 {
				eprintln!(
					"UPAD({}): {} [{}]",
					flags_size < expected_size,
					final_rf.len(),
					fd_map.get(&fd).cloned().unwrap_or_default(),
				);
			} else {
				eprintln!(
					"PADD({}): {}/{} [{}]",
					flags_size < expected_size,
					final_rf.len() - padded_bytes,
					padded_bytes,
					fd_map.get(&fd).cloned().unwrap_or_default(),
				);
			}
		}

		if read_file_header.starts_with(&[0xC4, 0x00, 0x24, 0x02, 0xE8, 0xEF, 0x24, 0x02]) {
			eprintln!("  -> header type: C4....");
		} else {
			eprintln!("  -> header type: {}", read_file_header[0]);
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
				if SHOULD_LOG_JSON() {
					error!(
						?cause,
						id = "dgswfp::generate::open_file_parse_failure",
						packet = format!("{packet:02x?}"),
						"Failed to parse open file request, so cannot determine correct nagle length!",
					);
				} else {
					error!(
						"\n{:?}",
						add_context_to(
							miette!(
								"Failed to parse open file request, so cannot determine correct nagle length!"
							),
							[cause.into()].into_iter(),
						),
					);
				}

				std::process::exit(GENERATE_NAGLE_FAILURE);
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

		return Some(
			usize::try_from(parsed_data.body().block_size()).unwrap_or(usize::MAX)
				* usize::try_from(parsed_data.body().block_count()).unwrap_or(usize::MAX),
		);
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

	None
}

fn string_to_hex_bytes(data: &str) -> Vec<u8> {
	let mut result = Vec::with_capacity(data.len() / 2);
	for chunk in data.chars().collect::<Vec<_>>().chunks(2) {
		let new_byte =
			u8::from_str_radix(&format!("{}{}", chunk[0], chunk[1]), 16).expect("bad byte!");
		result.push(new_byte);
	}
	result
}

/// Validate that the PCAP file exists, before continuing.
fn validate_exists(pcap_path: &Path) -> String {
	if !pcap_path.exists() || !pcap_path.is_file() {
		if SHOULD_LOG_JSON() {
			error!(
				id = "dgswfp::padlog::no_source_pcap",
				pcap.path = %pcap_path.display(),
				"Source PCAP is not an existing file, cannot parse!",
			);
		} else {
			error!(
				"\n{:?}",
				add_context_to(
					miette!("cannot parse a PCAP that is not an existing file"),
					[miette!(
						"Please ensure the PCAP location you specified is correct: {}",
						pcap_path.display(),
					)]
					.into_iter(),
				),
			);
		}

		std::process::exit(GENERATE_PCAP_DOES_NOT_EXIST);
	}

	let Some(pth) = pcap_path.to_str() else {
		if SHOULD_LOG_JSON() {
			error!(
				id = "dgswfp::padlog::path_not_utf8",
				pcap.path = %pcap_path.display(),
				"Source PCAP path must be representable as a UTF-8 string!",
			);
		} else {
			error!(
				"\n{:?}",
				add_context_to(
					miette!("cannot parse PCAP whose path is not fully UTF-8!"),
					[miette!(
						"Please move the PCAP file into a UTF-8 compatible path: {}",
						pcap_path.display(),
					)]
					.into_iter(),
				),
			);
		}

		std::process::exit(GENERATE_CANT_OPEN_PCAP);
	};
	pth.to_owned()
}
