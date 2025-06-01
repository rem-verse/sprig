//! API's for interacting with `/dbg/mem_dump.cgi`, a page for live
//! accessing the memory of the memory on the main chip of the MION
//! devices.
//!
//! Prefer this over `dbytes` over telnet, as there are some bytes that
//! cannot be read over telnet, that can be read with this CGI interface.

use crate::{
	errors::{CatBridgeError, NetworkError, NetworkParseError},
	mion::{
		cgis::{AUTHZ_HEADER, encode_url_parameters},
		proto::cgis::MIONCGIErrors,
	},
};
use bytes::{Bytes, BytesMut};
use fnv::FnvHashMap;
use futures::{StreamExt, future::Either};
use reqwest::{Client, Response, Version};
use std::{
	fmt::Display,
	net::Ipv4Addr,
	ops::Deref,
	sync::atomic::{AtomicU8, Ordering as AtomicOrdering},
	time::Duration,
};
use tokio::{
	sync::mpsc::{
		Receiver as BoundedReceiver, Sender as BoundedSender, channel as bounded_channel,
	},
	time::timeout,
};
use tracing::debug;

const MEMORY_MAX_ADDRESS: usize = 0xFFFF_FE00;
const TABLE_START_SIGIL: &str = "<table border=0 cellspacing=3 cellpadding=3>";
const TABLE_END_SIGIL: &str = "</table>";
const MAX_RETRIES: u8 = 10;
const BACKOFF_SLEEP_SECONDS: u64 = 10;
const MEMORY_TIMEOUT_SECONDS: u64 = 30;
const MAX_MEMORY_CONCURRENCY: usize = 4;

/// Dump the existing memory for a MION.
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn dump_memory(
	mion_ip: Ipv4Addr,
	resume_at: Option<usize>,
	early_stop_at: Option<usize>,
) -> Result<Bytes, CatBridgeError> {
	let mut memory_buffer = BytesMut::with_capacity(0xFFFF_FFFF);
	dump_memory_with_raw_client(
		&Client::default(),
		mion_ip,
		resume_at,
		early_stop_at,
		|bytes: Vec<u8>| {
			memory_buffer.extend_from_slice(&bytes);
		},
	)
	.await?;
	Ok(memory_buffer.freeze())
}

/// Dump the existing memory for a MION allowing you to write as
/// dumps happen.
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn dump_memory_with_writer<FnTy>(
	mion_ip: Ipv4Addr,
	resume_at: Option<usize>,
	early_stop_at: Option<usize>,
	callback: FnTy,
) -> Result<(), CatBridgeError>
where
	FnTy: FnMut(Vec<u8>) + Send + Sync,
{
	dump_memory_with_raw_client(
		&Client::default(),
		mion_ip,
		resume_at,
		early_stop_at,
		callback,
	)
	.await
}

/// Perform a memory dump request, but with an already existing HTTP client.
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn dump_memory_with_raw_client<FnTy>(
	client: &Client,
	mion_ip: Ipv4Addr,
	resume_at: Option<usize>,
	early_stop_at: Option<usize>,
	buff_callback: FnTy,
) -> Result<(), CatBridgeError>
where
	FnTy: FnMut(Vec<u8>) + Send + Sync,
{
	let (stop_requests_sender, mut stop_requests_consumer) = bounded_channel(1);
	let retry_counter = AtomicU8::new(0);

	let start_address = resume_at.unwrap_or(0);
	let (page_results_sender, page_results_consumer) = bounded_channel(512);

	let retry_counter_ref = &retry_counter;
	let sender_ref = &page_results_sender;
	// This will make memory page requests concurrently in chunks of
	// `MAX_MEMORY_CONCURRENCY`, with each task handling it's own retry
	// logic.
	//
	// If requests start throwing errors, the receiving channel will send
	// a message to `stop_requests_sender` which will shut everything down.
	let buffered_stream_future = futures::stream::iter(
		(start_address..=early_stop_at.unwrap_or(MEMORY_MAX_ADDRESS)).step_by(512),
	)
	.map(|page_start| async move {
		loop {
			if !do_memory_page_fetch(client, mion_ip, page_start, retry_counter_ref, sender_ref)
				.await
			{
				break;
			}
		}
	})
	.buffered(MAX_MEMORY_CONCURRENCY)
	.collect::<Vec<()>>();

	// As requests finish, they may not necissarily be in order.
	// We need to reorder them to ensure they're called back in a
	// serial order.
	let join_handle = do_memory_page_ordering(
		page_results_consumer,
		stop_requests_sender,
		start_address,
		buff_callback,
	);

	{
		let recv_future = stop_requests_consumer.recv();

		futures::pin_mut!(buffered_stream_future);
		futures::pin_mut!(recv_future);

		let (select, _joined) = futures::future::join(
			// Wait for all requests to finish, or a stop signal caused by an error.
			futures::future::select(buffered_stream_future, recv_future),
			// Wait for all of our ordering to finish too.
			join_handle,
		)
		.await;
		match select {
			Either::Right((error, _)) => {
				if let Some(cause) = error {
					return Err(cause);
				}
			}
			Either::Left(_) => {}
		}
	}

	// Double check we didn't get an error ordering all the pages.
	if let Ok(cause) = stop_requests_consumer.try_recv() {
		return Err(cause);
	}

	Ok(())
}

async fn do_memory_page_ordering<FnTy>(
	mut results: BoundedReceiver<Result<(usize, Vec<u8>), CatBridgeError>>,
	stopper: BoundedSender<CatBridgeError>,
	start_at: usize,
	mut callback: FnTy,
) where
	FnTy: FnMut(Vec<u8>),
{
	let mut out_of_order_cache: FnvHashMap<usize, Vec<u8>> = FnvHashMap::default();
	let mut looking_for_page = start_at;

	while looking_for_page <= MEMORY_MAX_ADDRESS {
		if let Some(data) = out_of_order_cache.remove(&looking_for_page) {
			callback(data);
			looking_for_page += 512;
			continue;
		}

		let Some(page_result) = results.recv().await else {
			_ = stopper.send(CatBridgeError::ClosedChannel).await;
			break;
		};
		match page_result {
			Ok((addr, data)) => {
				out_of_order_cache.insert(addr, data);
			}
			Err(cause) => {
				_ = stopper.send(cause).await;
				break;
			}
		}
	}
}

async fn do_memory_page_fetch(
	client: &Client,
	mion_ip: Ipv4Addr,
	page_start: usize,
	retry_counter: &AtomicU8,
	result_stream: &BoundedSender<Result<(usize, Vec<u8>), CatBridgeError>>,
) -> bool {
	let start_addr = format!("{page_start:08X}");
	debug!(
		bridge.ip = %mion_ip,
		addr = %start_addr,
		"Performing memory page fetch",
	);

	let timeout_response = timeout(
		Duration::from_secs(MEMORY_TIMEOUT_SECONDS),
		do_raw_memory_request(client, mion_ip, &[("start_addr", start_addr)]),
	)
	.await;

	let Ok(potential_response) = timeout_response else {
		if retry_counter.fetch_add(1, AtomicOrdering::AcqRel) > MAX_RETRIES {
			_ = result_stream
				.send(Err(NetworkError::Timeout(Duration::from_secs(
					MEMORY_TIMEOUT_SECONDS,
				))
				.into()))
				.await;
			return false;
		}
		debug!(bridge.ip = %mion_ip, "Slamming Memory dump too hard... backing off for a bit");
		tokio::time::sleep(Duration::from_secs(BACKOFF_SLEEP_SECONDS)).await;
		return true;
	};
	let response = match potential_response {
		Ok(value) => value,
		Err(cause) => {
			if retry_counter.fetch_add(1, AtomicOrdering::AcqRel) > MAX_RETRIES {
				_ = result_stream.send(Err(cause.into())).await;
				return false;
			}
			debug!(bridge.ip = %mion_ip, "Slamming Memory dump too hard... backing off for a bit");
			tokio::time::sleep(Duration::from_secs(BACKOFF_SLEEP_SECONDS)).await;
			return true;
		}
	};

	let status = response.status().as_u16();
	let timeout_body_result = timeout(
		Duration::from_secs(MEMORY_TIMEOUT_SECONDS),
		response.bytes(),
	)
	.await;
	let Ok(body_result) = timeout_body_result else {
		if retry_counter.fetch_add(1, AtomicOrdering::AcqRel) > MAX_RETRIES {
			_ = result_stream
				.send(Err(NetworkError::Timeout(Duration::from_secs(
					MEMORY_TIMEOUT_SECONDS,
				))
				.into()))
				.await;
			return false;
		}
		debug!(bridge.ip = %mion_ip, "Slamming Memory dump too hard... backing off for a bit");
		tokio::time::sleep(Duration::from_secs(BACKOFF_SLEEP_SECONDS)).await;
		return true;
	};

	retry_counter.store(0, AtomicOrdering::Release);
	if status != 200 {
		if let Ok(body) = body_result {
			_ = result_stream
				.send(Err(MIONCGIErrors::UnexpectedStatusCode(status, body).into()))
				.await;
			return false;
		}

		_ = result_stream
			.send(Err(MIONCGIErrors::UnexpectedStatusCodeNoBody(status).into()))
			.await;
		return false;
	}
	let read_body_bytes = match body_result.map_err(NetworkError::HTTP) {
		Ok(value) => value,
		Err(cause) => {
			_ = result_stream.send(Err(cause.into())).await;
			return false;
		}
	};
	let body_as_string =
		match String::from_utf8(read_body_bytes.into()).map_err(NetworkParseError::Utf8Expected) {
			Ok(value) => value,
			Err(cause) => {
				_ = result_stream.send(Err(cause.into())).await;
				return false;
			}
		};

	process_received_page(page_start, result_stream, &body_as_string).await
}

async fn process_received_page(
	page_start: usize,
	result_stream: &BoundedSender<Result<(usize, Vec<u8>), CatBridgeError>>,
	body_as_string: &str,
) -> bool {
	let table = match extract_memory_table_body(body_as_string) {
		Ok(value) => value,
		Err(cause) => {
			_ = result_stream.send(Err(cause.into())).await;
			return false;
		}
	};
	let mut page_of_bytes = Vec::with_capacity(512);
	for table_row in table.split("<tr>").skip(3) {
		for table_column in table_row
			.trim()
			.trim_end_matches("</tbody>")
			.trim_end()
			.trim_end_matches("</tr>")
			.trim_end()
			.replace("</td>", "")
			.split("<td>")
			.skip(3)
		{
			if table_column.trim().len() != 2 {
				_ = result_stream
					.send(Err(MIONCGIErrors::HtmlResponseBadByte(
						table_column.to_owned(),
					)
					.into()))
					.await;
				return false;
			}
			let byte = match u8::from_str_radix(table_column.trim(), 16)
				.map_err(|_| MIONCGIErrors::HtmlResponseBadByte(table_column.to_owned()))
			{
				Ok(value) => value,
				Err(cause) => {
					_ = result_stream.send(Err(cause.into())).await;
					return false;
				}
			};
			page_of_bytes.push(byte);
		}
	}

	_ = result_stream.send(Ok((page_start, page_of_bytes))).await;
	false
}

fn extract_memory_table_body(body: &str) -> Result<String, MIONCGIErrors> {
	let start = body
		.find(TABLE_START_SIGIL)
		.ok_or_else(|| MIONCGIErrors::HtmlResponseMissingMemoryDumpSigil(body.to_owned()))?;
	let body_minus_start = &body[start + TABLE_START_SIGIL.len()..];
	let end = body_minus_start
		.find(TABLE_END_SIGIL)
		.ok_or_else(|| MIONCGIErrors::HtmlResponseMissingMemoryDumpSigil(body.to_owned()))?;

	Ok(body_minus_start[..end].to_owned())
}

/// Perform a raw request on the MION board's `eeprom_dump.cgi` page.
///
/// *note: you probably want to call one of the actual methods, as this is
/// basically just a thin wrapper around an HTTP Post Request. Not doing much
/// else more. A lot of it requires that you set things up correctly.*
///
/// ## Errors
///
/// - If we cannot make an HTTP request to the MION Request.
/// - If we fail to encode your parameters into a request body.
pub async fn do_raw_memory_request(
	client: &Client,
	mion_ip: Ipv4Addr,
	url_parameters: &[(impl Deref<Target = str>, impl Display)],
) -> Result<Response, NetworkError> {
	Ok(client
		.post(format!("http://{mion_ip}/dbg/mem_dump.cgi"))
		.version(Version::HTTP_11)
		.header("authorization", format!("Basic {AUTHZ_HEADER}"))
		.header("content-type", "application/x-www-form-urlencoded")
		.header(
			"user-agent",
			format!("cat-dev/{}", env!("CARGO_PKG_VERSION")),
		)
		.body::<String>(encode_url_parameters(url_parameters))
		.send()
		.await?)
}
