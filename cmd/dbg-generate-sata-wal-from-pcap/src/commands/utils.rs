//! Utilities that are common across multiple commands.

use crate::{
	SHOULD_LOG_JSON,
	exit_codes::{
		ARGV_PCAP_DOES_NOT_EXIST, ARGV_PCAP_PATH_NOT_UTF8, UTILS_CANNOT_SPAWN_TSHARK,
		UTILS_TSHARK_READ_FAILURE,
	},
	utils::add_context_to,
};
use bytes::{BufMut, Bytes, BytesMut};
use miette::miette;
use rtshark::{Metadata as RtMetadata, RTShark, RTSharkBuilder};
use std::{iter::Iterator, path::Path};
use tracing::{debug, error};

/// Validate that the PCAP file exists, and is stored on a UTF-8 path.
pub fn validate_pcap_path_constraints(pcap_path: &Path) -> String {
	if !pcap_path.exists() || !pcap_path.is_file() {
		if SHOULD_LOG_JSON() {
			error!(
				id = "dgswfp::utils::no_source_pcap",
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

		std::process::exit(ARGV_PCAP_DOES_NOT_EXIST);
	}

	let Some(existing_path) = pcap_path.to_str() else {
		if SHOULD_LOG_JSON() {
			error!(
				id = "dgswfp::utils::pcap_path_not_utf8",
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

		std::process::exit(ARGV_PCAP_PATH_NOT_UTF8);
	};

	existing_path.to_owned()
}

/// A stream over all the packets with data happening on a particular port.
///
/// This for now is just a thin wrapper around an rtshark iterator, and stream
/// that will filter out any packet that is not on the correct port. Then it
/// will map and return just the actual payload along with whether it's
/// incoming, or outgoing.
pub struct PacketsWithDataOnPort {
	/// If we've already finished our stream.
	finished: bool,
	/// The port to filter data on.
	port: u16,
	/// The underlying command we will end up iterating ontop of.
	underlying_command: RTShark,
}

impl PacketsWithDataOnPort {
	/// Create a new iterator over all the packets coming into/going out of a
	/// port.
	#[must_use]
	pub fn new(pcap: &str, port: u16) -> Self {
		let underlying_command = match RTSharkBuilder::builder()
			.input_path(pcap)
			.disable_protocol("ALL")
			.enable_protocol("eth")
			.enable_protocol("ip")
			.enable_protocol("tcp")
			.display_filter(&format!("tcp.port == {port} && data.len > 0"))
			.spawn()
		{
			Ok(builder) => builder,
			Err(cause) => {
				if SHOULD_LOG_JSON() {
					error!(
						id = "dgswfp::utils::pcap_spawn_failure",
						pcap.path = pcap,
						"failed to spawn tshark, and read from PCAP/PCAPNG.",
					);
				} else {
					error!(
						"\n{:?}",
						add_context_to(
							miette!("Failed to spawn TSHARK, and read from PCAP/PCAPNG."),
							[miette!("{cause:?}")].into_iter(),
						),
					);
				}

				std::process::exit(UTILS_CANNOT_SPAWN_TSHARK);
			}
		};

		Self {
			finished: false,
			port,
			underlying_command,
		}
	}
}

impl Iterator for PacketsWithDataOnPort {
	type Item = PacketOnPort;

	fn next(&mut self) -> Option<Self::Item> {
		if self.finished {
			return None;
		}

		loop {
			match self.underlying_command.read() {
				Ok(Some(pkt)) => {
					// Just cause we have a packet doesn't mean it can be used...
					let Some(tcp_layer) = pkt.layer_name("tcp") else {
						debug!("packet is missing TCP layer! skipping...");
						continue;
					};

					let Some(srcport) = tcp_layer
						.metadata("tcp.srcport")
						.and_then(|val| val.value().parse::<u16>().ok())
					else {
						debug!("packet is missing `tcp.srcport`");
						continue;
					};
					let Some(dstport) = tcp_layer
						.metadata("tcp.dstport")
						.and_then(|val| val.value().parse::<u16>().ok())
					else {
						debug!("packet is missing `tcp.dstport`");
						continue;
					};

					if srcport != self.port && dstport != self.port {
						debug!("packet is not incoming to the correct port");
						continue;
					}
					let is_request = dstport == self.port;

					let Some(sid) = tcp_layer
						.metadata("tcp.stream")
						.and_then(|val| val.value().parse::<u64>().ok())
					else {
						debug!("packet is missing `tcp.stream`");
						continue;
					};
					let Some(payload) = tcp_layer
						.metadata("tcp.payload")
						.map(RtMetadata::raw_value)
						.map(string_to_hex_bytes)
					else {
						debug!("packet is missing `tcp.payload`");
						continue;
					};

					return Some(PacketOnPort::new(payload, is_request, sid));
				}
				Ok(None) => {
					self.finished = true;
					return None;
				}
				Err(cause) => {
					if SHOULD_LOG_JSON() {
						error!(
							id = "dgswfp::utils::pcap_read_failure",
							"failed to read from pcap",
						);
					} else {
						error!(
							"\n{:?}",
							add_context_to(
								miette!("Failed to read data from the PCAP!"),
								[miette!("{cause:?}")].into_iter(),
							),
						);
					}

					std::process::exit(UTILS_TSHARK_READ_FAILURE);
				}
			}
		}
	}
}

/// An actual packet that has come in on a specific port.
#[derive(Debug)]
pub struct PacketOnPort {
	/// The actual data of the packet.
	data: Bytes,
	/// If the packet was a request (e.g. dstport is equal to port).
	is_request: bool,
	/// The ID of the stream this packet is on the port.
	stream_id: u64,
}

impl PacketOnPort {
	/// Create a new packet representation.
	#[must_use]
	pub const fn new(data: Bytes, is_request: bool, stream_id: u64) -> Self {
		Self {
			data,
			is_request,
			stream_id,
		}
	}

	#[must_use]
	pub const fn data(&self) -> &Bytes {
		&self.data
	}
	#[must_use]
	pub const fn is_request(&self) -> bool {
		self.is_request
	}
	#[must_use]
	pub const fn stream_id(&self) -> u64 {
		self.stream_id
	}
}

fn string_to_hex_bytes(data: &str) -> Bytes {
	let mut result = BytesMut::with_capacity(data.len() / 2);

	for chunk in data.chars().collect::<Vec<_>>().chunks(2) {
		let new_byte =
			u8::from_str_radix(&format!("{}{}", chunk[0], chunk[1]), 16).expect("bad byte!");
		result.put_u8(new_byte);
	}

	result.freeze()
}
