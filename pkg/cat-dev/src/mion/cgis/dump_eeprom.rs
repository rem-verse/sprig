//! API's for interacting with `/dbg/eeprom_dump.cgi`, a page for live
//! accessing the memory of the EEPROM on the MION devices.

use crate::{
	errors::NetworkError,
	mion::{cgis::do_simple_request, proto::cgis::MIONCGIErrors},
};
use bytes::{BufMut, Bytes, BytesMut};
use reqwest::{Client, Method};
use serde::Serialize;
use std::net::Ipv4Addr;
use tracing::debug;

const EEPROM_MAX_ADDRESS: usize = 0x1E00;
const TABLE_START_SIGIL: &str = "<table border=0 cellspacing=3 cellpadding=3>";
const TABLE_END_SIGIL: &str = "</table>";

/// Dump the existing EEPROM for a MION.
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn dump_eeprom(mion_ip: Ipv4Addr) -> Result<Bytes, NetworkError> {
	dump_eeprom_with_raw_client(&Client::default(), mion_ip).await
}

/// Perform an EEPROM DUMP request, but with an already existing HTTP client.
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn dump_eeprom_with_raw_client(
	client: &Client,
	mion_ip: Ipv4Addr,
) -> Result<Bytes, NetworkError> {
	let mut memory_buffer = BytesMut::with_capacity(0x2000);

	while memory_buffer.len() <= EEPROM_MAX_ADDRESS {
		debug!(
		  bridge.ip = %mion_ip,
		  address = %format!("{:04X}", memory_buffer.len()),
		  "Dumping eeprom memory area",
		);

		let body_as_string = do_raw_eeprom_request(
			client,
			mion_ip,
			&[("start_addr", format!("{:04X}", memory_buffer.len()))],
		)
		.await?;

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
					return Err(MIONCGIErrors::HtmlResponseBadByte(table_column.to_owned()).into());
				}
				memory_buffer
					.put_u8(u8::from_str_radix(table_column.trim(), 16).map_err(|_| {
						MIONCGIErrors::HtmlResponseBadByte(table_column.to_owned())
					})?);
			}
		}
	}

	Ok(memory_buffer.freeze())
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
pub async fn do_raw_eeprom_request<'key, 'value, UrlEncodableType>(
	client: &Client,
	mion_ip: Ipv4Addr,
	url_parameters: UrlEncodableType,
) -> Result<String, NetworkError>
where
	UrlEncodableType: Serialize,
{
	do_simple_request::<String>(
		client,
		Method::POST,
		format!("http://{mion_ip}/dbg/eeprom_dump.cgi"),
		Some(
			serde_urlencoded::to_string(&url_parameters)
				.map_err(MIONCGIErrors::FormDataEncodeError)?,
		),
	)
	.await
}
