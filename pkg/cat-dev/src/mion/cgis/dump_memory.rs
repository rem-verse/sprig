//! API's for interacting with `/dbg/mem_dump.cgi`, a page for live
//! accessing the memory of the memory on the main chip of the MION
//! devices.
//!
//! Prefer this over `dbytes` over telnet, as there are some bytes that
//! cannot be read over telnet, that can be read with this CGI interface.

use crate::{
	errors::{CatBridgeError, NetworkError, NetworkParseError},
	mion::cgis::AUTHZ_HEADER,
};
use bytes::{BufMut, Bytes, BytesMut};
use hyper::{
	body::to_bytes as read_http_body_bytes,
	client::{connect::Connect, Client},
	Body, Request, Response, Version,
};
use serde::Serialize;
use std::{net::Ipv4Addr, time::Duration};
use tokio::time::timeout;
use tracing::debug;

const MEMORY_MAX_ADDRESS: usize = 0xFFFF_FE00;
const TABLE_START_SIGIL: &str = "<table border=0 cellspacing=3 cellpadding=3>";
const TABLE_END_SIGIL: &str = "</table>";
const MAX_RETRIES: usize = 10;

/// Dump the existing memory for a MION.
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn dump_memory(mion_ip: Ipv4Addr) -> Result<Bytes, CatBridgeError> {
	dump_memory_with_raw_client(&Client::default(), mion_ip).await
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
pub async fn dump_memory_with_raw_client<ClientConnectorTy>(
	client: &Client<ClientConnectorTy>,
	mion_ip: Ipv4Addr,
) -> Result<Bytes, CatBridgeError>
where
	ClientConnectorTy: Clone + Connect + Send + Sync + 'static,
{
	let mut memory_buffer = BytesMut::with_capacity(0xFFFF_FFFF);

	let mut retry_counter = 0;
	while memory_buffer.len() <= MEMORY_MAX_ADDRESS {
		debug!(
		  bridge.ip = %mion_ip,
		  address = %format!("{:08X}", memory_buffer.len()),
		  "Dumping memory area",
		);

		let timeout_response = timeout(
			Duration::from_secs(30),
			do_raw_memory_request(
				client,
				mion_ip,
				&[("start_addr", format!("{:08X}", memory_buffer.len()))],
			),
		)
		.await;
		let Ok(potential_response) = timeout_response else {
			retry_counter += 1;
			if retry_counter > MAX_RETRIES {
				return Err(NetworkError::TimeoutError.into());
			}
			debug!(bridge.ip = %mion_ip, "Slamming Memory dump too hard... backing off for a bit");
			tokio::time::sleep(Duration::from_secs(10)).await;
			continue;
		};
		let response = match potential_response {
			Ok(value) => value,
			Err(cause) => {
				retry_counter += 1;
				if retry_counter > MAX_RETRIES {
					return Err(cause.into());
				}
				debug!(bridge.ip = %mion_ip, "Slamming Memory dump too hard... backing off for a bit");
				tokio::time::sleep(Duration::from_secs(10)).await;
				continue;
			}
		};

		let status = response.status().as_u16();
		let timeout_body_result = timeout(
			Duration::from_secs(30),
			read_http_body_bytes(response.into_body()),
		)
		.await;
		let Ok(body_result) = timeout_body_result else {
			retry_counter += 1;
			if retry_counter > MAX_RETRIES {
				return Err(NetworkError::TimeoutError.into());
			}
			debug!(bridge.ip = %mion_ip, "Slamming Memory dump too hard... backing off for a bit");
			tokio::time::sleep(Duration::from_secs(10)).await;
			continue;
		};

		retry_counter = 0;
		if status != 200 {
			if let Ok(body) = body_result {
				return Err(CatBridgeError::NetworkError(NetworkError::ParseError(
					NetworkParseError::UnexpectedStatusCode(status, body),
				)));
			}

			return Err(CatBridgeError::NetworkError(NetworkError::ParseError(
				NetworkParseError::UnexpectedStatusCodeNoBody(status),
			)));
		}
		let read_body_bytes = body_result.map_err(NetworkError::HyperError)?;
		let body_as_string = String::from_utf8(read_body_bytes.into())
			.map_err(NetworkParseError::InvalidDataNeedsUTF8)
			.map_err(NetworkError::ParseError)?;

		let table = extract_memory_table_body(&body_as_string)?;
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
					return Err(CatBridgeError::NetworkError(NetworkError::ParseError(
						NetworkParseError::HtmlResponseBadByte(table_column.to_owned()),
					)));
				}
				memory_buffer.put_u8(u8::from_str_radix(table_column.trim(), 16).map_err(
					|_| {
						NetworkError::ParseError(NetworkParseError::HtmlResponseBadByte(
							table_column.to_owned(),
						))
					},
				)?);
			}
		}
	}

	Ok(memory_buffer.freeze())
}

fn extract_memory_table_body(body: &str) -> Result<String, CatBridgeError> {
	let start = body.find(TABLE_START_SIGIL).ok_or_else(|| {
		NetworkError::ParseError(NetworkParseError::HtmlResponseMissingMemoryDumpSigil(
			body.to_owned(),
		))
	})?;
	let body_minus_start = &body[start + TABLE_START_SIGIL.len()..];
	let end = body_minus_start.find(TABLE_END_SIGIL).ok_or_else(|| {
		NetworkError::ParseError(NetworkParseError::HtmlResponseMissingMemoryDumpSigil(
			body.to_owned(),
		))
	})?;

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
pub async fn do_raw_memory_request<'key, 'value, ClientConnectorTy, UrlEncodableType>(
	client: &Client<ClientConnectorTy>,
	mion_ip: Ipv4Addr,
	url_parameters: UrlEncodableType,
) -> Result<Response<Body>, NetworkError>
where
	ClientConnectorTy: Clone + Connect + Send + Sync + 'static,
	UrlEncodableType: Serialize,
{
	Ok(client
		.request(
			Request::post(format!("http://{mion_ip}/dbg/mem_dump.cgi"))
				.version(Version::HTTP_11)
				.header("authorization", format!("Basic {AUTHZ_HEADER}"))
				.header("content-type", "application/x-www-form-urlencoded")
				.header(
					"user-agent",
					format!("cat-dev/{}", env!("CARGO_PKG_VERSION")),
				)
				.body(
					serde_urlencoded::to_string(&url_parameters)
						.map_err(NetworkParseError::FormDataEncodeError)?
						.into(),
				)?,
		)
		.await?)
}
