//! The server implementation for handling ATAPI Emulation.

use crate::{
	errors::{APIError, CatBridgeError, FSError},
	fsemul::{HostFilesystem, dlf::DiskLayoutFile},
	net::{
		DEFAULT_CAT_DEV_CHUNK_SIZE, DEFAULT_CAT_DEV_SLOWDOWN,
		additions::{RequestIDLayer, StreamIDLayer},
		server::{
			Router, TCPServer,
			requestable::{Body, State},
		},
	},
};
use bytes::{BufMut, Bytes, BytesMut};
use local_ip_address::local_ip;
use std::{
	net::{IpAddr, Ipv4Addr, SocketAddrV4},
	time::Duration,
};
use tokio::{
	fs::{File, read as fs_read},
	io::{AsyncReadExt, AsyncSeekExt, SeekFrom},
};
use tower::ServiceBuilder;
use tracing::debug;

/// The default port to use for hosting the ATAPI Server.
pub const DEFAULT_ATAPI_PORT: u16 = 7974_u16;

/// Create an ATAPI Server that is capable of serving ATAPI responses to clients.
///
/// ## Errors
///
/// If we cannot find a host ip to bind too, or cannot spin up the workers for
/// the server.
#[allow(
	// TODO(mythra): we should probably extract this out into a builder
	// pattern some day. That day is not today.
	clippy::too_many_arguments,
)]
pub async fn create_atapi_server(
	host_filesystem: HostFilesystem,
	address: Option<Ipv4Addr>,
	port: Option<u16>,
	cat_dev_sleep_override: Option<Duration>,
	fully_disable_cat_dev_sleep: bool,
	chunk_override: Option<usize>,
	fully_disable_chunk_override: bool,
	trace_during_debug: bool,
) -> Result<TCPServer<HostFilesystem>, CatBridgeError> {
	let Some(ip) = address.or_else(|| {
		// This always returns an ipv4 address, but is still returning
		// an ip address for legacy reasons.
		local_ip().ok().map(|ip| match ip {
			IpAddr::V4(v4) => v4,
			IpAddr::V6(_v6) => unreachable!(),
		})
	}) else {
		return Err(APIError::NoHostIpFound.into());
	};
	let bound_address = SocketAddrV4::new(ip, port.unwrap_or(DEFAULT_ATAPI_PORT));

	let mut router = Router::<HostFilesystem>::new();
	router.fallback_handler(temporary_fallback_handle_all)?;

	let mut server = TCPServer::new_with_state(
		"atapi",
		bound_address,
		router,
		(None, None),
		12,
		host_filesystem,
		trace_during_debug,
	)
	.await?;
	if trace_during_debug {
		server.layer_initial_service(
			ServiceBuilder::new()
				.layer(RequestIDLayer::new("atapi".to_owned()))
				.layer(StreamIDLayer),
		);
	} else {
		server.layer_initial_service(
			ServiceBuilder::new().layer(RequestIDLayer::new("atapi".to_owned())),
		);
	}

	server.set_chunk_output_at_size(if fully_disable_chunk_override {
		None
	} else if let Some(over_ride) = chunk_override {
		Some(over_ride)
	} else {
		Some(DEFAULT_CAT_DEV_CHUNK_SIZE)
	});
	server.set_cat_dev_slowdown(if fully_disable_cat_dev_sleep {
		None
	} else {
		Some(cat_dev_sleep_override.unwrap_or(DEFAULT_CAT_DEV_SLOWDOWN))
	});

	Ok(server)
}

async fn temporary_fallback_handle_all(
	State(fs): State<HostFilesystem>,
	Body(packet): Body<Bytes>,
) -> Result<Option<Bytes>, CatBridgeError> {
	match &packet[..2] {
		[0x3, _] => {
			debug!("Would have sent 32 bytes of various descriptions back... not sure which...");
		}
		[0xCF, 0x80] => {
			debug!("ATAPI Event packet sent");
		}
		[0xF0, _] => {
			return Ok(Some(Bytes::from(vec![0x0; 4])));
		}
		[0xF1, 0x00 | 0x02] => {
			// I think this is just random data?
			return Ok(Some(Bytes::from(vec![0x69; 32])));
		}
		[0xF1, 0x01 | 0x03] => {
			debug!("Unknown 0xF1 packet, doesn't do anything on the network...");
		}
		[0xF2, _] => {
			debug!("Got unknown 0xF2 packet: [{packet:02X?}]");
		}
		[0xF3, 0x00] => {
			return handle_read_dlf(packet, &fs).await;
		}
		[0xF3, 0x01] => {
			let mut data = BytesMut::with_capacity(32);
			data.extend_from_slice(b"PC SATA EMUL");
			data.extend_from_slice(&[0_u8; 20]);
			return Ok(Some(data.freeze()));
		}
		[0xF3, 0x02 | 0x03] | [0xF5 | 0xF7, _] => {
			debug!("Sending empty 32 bytes!");
			return Ok(Some(Bytes::from(vec![0x0; 32])));
		}
		[0xF6, _] => {
			if packet[1] & 3 != 0 {
				debug!("F6 second byte & 3 != 0, not sending reply!");
			} else {
				let mut data = BytesMut::with_capacity(4);
				data.put_u32_le(1);
				debug!("Sent F6 reply!");
				return Ok(Some(data.freeze()));
			}
		}
		[0x12, _] => {
			debug!("Would have sent 96 bytes of various descriptions back... not sure which...");
		}
		_ => {}
	}

	Ok(None)
}

async fn handle_read_dlf(
	packet: Bytes,
	host_filesystem: &HostFilesystem,
) -> Result<Option<Bytes>, CatBridgeError> {
	let read_address = u128::from(u32::from_be_bytes([
		packet[0x4],
		packet[0x5],
		packet[0x6],
		packet[0x7],
	])) << 11_u128;
	let read_length = u128::from(u32::from_be_bytes([
		packet[0x8],
		packet[0x9],
		packet[0xA],
		packet[0xB],
	])) << 11_u128;
	let rl_as_usize =
		usize::try_from(read_length).map_err(|_| CatBridgeError::UnsupportedBitsPerCore)?;

	debug!(
		atapi.packet_type = "read_address",
		atapi.read_address.address = %read_address,
		atapi.read_address.length = %read_length,
		"Handling atapi read request!"
	);

	let bytes_of_dlf = fs_read(host_filesystem.ppc_boot_dlf_path().await?)
		.await
		.map_err(FSError::from)?;
	let dlf = DiskLayoutFile::try_from(Bytes::from(bytes_of_dlf))?;

	if let Some((path, offset)) = dlf.get_path_and_offset_for_file(read_address).await {
		// Read the file contents...
		let buff = {
			let mut handle = File::open(&path).await.map_err(FSError::from)?;
			handle
				.seek(SeekFrom::Start(offset))
				.await
				.map_err(FSError::from)?;

			let mut file_buff = BytesMut::zeroed(rl_as_usize);
			let mut bytes_read = 0;
			while bytes_read < rl_as_usize {
				let read_this_go = handle
					.read(&mut file_buff[bytes_read..])
					.await
					.map_err(FSError::IO)?;
				// EOF, rest of the buff is already 0's, so no need to pad.
				if read_this_go == 0 {
					break;
				}
				bytes_read += read_this_go;
			}

			file_buff
		};

		// Send!
		Ok(Some(buff.freeze()))
	} else {
		Ok(Some(BytesMut::zeroed(rl_as_usize).freeze()))
	}
}

// KNOWN PACKET HEADERS
//
//  - [0x3]
//    - send 32 bytes of various describes, not quite sure
//  - [0x12]
//    - sends 96 bytes, seems to have some random spattering of fields
//  - [0xCF, 0x80] -> Triggers Events in FSEmul, probably just call "EVentTrigger"
//  - [0xF0] -> send back 4, 0x0 bytes
//  - [0xF1]
//    - [0xF1, 0x00]
//      - seems to literally send 32 bytes of random data.... cool
//    - [0xF1, 0x02]
//      - seems to literally send 32 bytes of random data.... cool
//    - [0xF1, 0x01] || [0xF1, 0x03]
//      - seems to do nothing on hthe network
//  - [0xF2]
//    - [0xF2, 0x00]
//    - [0xF2, 0x01]
//    - [0xF2, 0x02]
//    - [0xF2, 0x03]
//      -> ??? calls a dynamically allocated thing
//    - [0xF2, 0x06]
//    - [0xF2, 0x07]
//      -> ??? calls a dynamically allocated thing
//      -> for f207 looks like we don't send _anything_ back by default
//      -> seems some paths check for Dvdroot, so maybe dvd stuff?
//  - [0xF3]
//    - [0xF3, 0x0] -> seems to actually be real "read file"
//      - not quite sure exactly how to parse this yet, but these are examples:
//        - first 4 bytes are "packet id"
//        - second 4 bytes are "read address" (calculate by: cast to u128 `<< 11`)
//        - last 4 bytes are "read length" (calculate by: cast to u128 `<< 11`)
//      - logs seem to indicate we:
//         1. read a dlf file (dlf file is populated in cafe-tmp how get?)
//         2. use that to get a max read address
//         3. read from the file
//         4. then pad
//        see logs below:
//          - `CSataProcessor::could not get lead out from dlffileobj {error code}`
//          - `CSataProcessor::requested read address 0x%I64x is out of bounds.`
//          - `CSataProcessor::could not read from file`
//          - `CSataProcessor::padding`
//          - `CSataProcessor::error writing to MION port`
//          - `CSataProcessor::wrote %d bytes`
//    - [0xF3, 0x1] -> send back "PC SATA EMUL" + 20 0's
//    - [0xF3, 0x2] || [0xF3, 0x3] -> send back 32 0's - seems this is always set to 0. maybe a kind of ping?
//  - [0xF5]
//    - send 32 0's
//  - [0xF6]
//    - second byte &3 != 0 -> doesn't send reply
//    - if not send what looks to be `1` encoded as 4 bytes
//  - [0xF7]
//    - send 32 0's
