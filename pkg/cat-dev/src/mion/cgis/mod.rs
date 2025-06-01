//! CGI's that are available to interact with the MION on.
//!
//! These various CGI web pages you can interact with normally on the web.

/// HTTP Basic authorization header.
///
/// This gets passed as the header:
/// `Authorization: Basic bWlvbjovTXVsdGlfSS9PX05ldHdvcmsv`
///
/// Given this is http basic auth, you can decode this string as:
/// `mion:/Multi_I/O_Network/`
///
/// Which means the username is: `mion`, and the password is:
/// `/Multi_I/O_Network/`.
const AUTHZ_HEADER: &str = "bWlvbjovTXVsdGlfSS9PX05ldHdvcmsv";

mod control;
mod dump_eeprom;
mod dump_memory;
mod errors;
mod setup;
mod signal_get;
mod status;
mod update;

pub use control::*;
pub use dump_eeprom::*;
pub use dump_memory::*;
pub use errors::*;
pub use setup::*;
pub use signal_get::*;
pub use status::*;
pub use update::*;

use crate::{
	errors::{NetworkError, NetworkParseError},
	mion::proto::cgis::MIONCGIErrors,
};
use bytes::Bytes;
use form_urlencoded::byte_serialize;
use reqwest::{Body, Client, Method, Response, Version};
use std::{fmt::Display, ops::Deref};
use tracing::{field::valuable, warn};

/// Perform a request that attempts to remove all the logic for the 'simple'
/// request cases.
///
/// Most MION HTTP Requests follow a simple format:
///
/// - Use HTTP Version 1.1.
/// - Authenticate with the common authorization header.
/// - Content Type is: `application/x-www-form-urlencoded`.
/// - Only Response Status Code that is successful is 200.
/// - There is a response body, and it is a UTF-8 String.
///
/// If your request meets all these requirements, then congrats! You can use
/// this method to implement it. Cutting down on the logic you have to
/// implement by a ton! Otherwise, you'll have to implement bits, and pieces
/// yourself.
async fn do_simple_request<BodyTy>(
	client: &Client,
	method: Method,
	url: String,
	body: Option<BodyTy>,
) -> Result<String, NetworkError>
where
	BodyTy: Into<Body>,
{
	let mut req = client
		.request(method, url)
		.version(Version::HTTP_11)
		.header("authorization", format!("Basic {AUTHZ_HEADER}"))
		.header("content-type", "application/x-www-form-urlencoded")
		.header("user-agent", concat!("cat-dev/", env!("CARGO_PKG_VERSION")));
	if let Some(body) = body {
		req = req.body(body);
	}
	let response_body =
		assert_status_and_read_body(200, req.send().await.map_err(NetworkError::HTTP)?).await?;

	Ok(String::from_utf8(response_body.into()).map_err(NetworkParseError::Utf8Expected)?)
}

fn encode_url_parameters(parameters: &[(impl Deref<Target = str>, impl Display)]) -> String {
	let mut buff = String::new();
	let mut first = true;

	for (param_key, param_value) in parameters {
		if !first {
			buff.push('&');
		}
		first = false;
		buff.extend(byte_serialize(param_key.as_bytes()));
		buff.push('=');
		buff.extend(byte_serialize(format!("{param_value}").as_bytes()));
	}

	buff
}

/// Assert that a response status code is a specific code, and get the body.
///
/// ## Errors
///
/// If the status code did not match the expected result.
async fn assert_status_and_read_body(
	needed_status: u16,
	response: Response,
) -> Result<Bytes, NetworkError> {
	let status = response.status().as_u16();
	let body_result = response.bytes().await.map_err(NetworkError::HTTP);
	if status != needed_status {
		if let Ok(body) = body_result {
			return Err(MIONCGIErrors::UnexpectedStatusCode(status, body).into());
		}

		return Err(MIONCGIErrors::UnexpectedStatusCodeNoBody(status).into());
	}

	body_result
}

/// Parse out a result status from an HTML page.
///
/// If your page ends up rendering up results like: `RESULT:OK` when things are
/// successful, you can use this method to get that result information.
fn parse_result_from_body(body: &str, operation_name: &str) -> Result<bool, MIONCGIErrors> {
	let start_tag_location = body
		.find("<body>")
		.map(|num| num + 6)
		.or(
			// Some pages don't have a start body tag, because MION's HTML stack
			// is absolute trash.
			body.find("<HTML>").map(|num| num + 6),
		)
		.ok_or_else(|| MIONCGIErrors::HtmlResponseMissingBody(body.to_owned()))?;
	let body_without_start_tag = body.split_at(start_tag_location).1;
	let end_tag_location = body_without_start_tag
		.find("</body>")
		.ok_or_else(|| MIONCGIErrors::HtmlResponseMissingBody(body.to_owned()))?;
	let just_inner_body = body_without_start_tag.split_at(end_tag_location).0;
	let without_newlines_or_extra_tags = just_inner_body
		.replace('\n', "")
		.replace("<CENTER>", "")
		.replace("</CENTER>", "");

	let mut was_successful = false;
	let mut returned_result_code = "";
	let mut log_lines = Vec::with_capacity(0);
	let mut extra_lines = Vec::with_capacity(0);
	for line in without_newlines_or_extra_tags
		.split("<br>")
		.fold(Vec::new(), |mut accum, item| {
			accum.extend(item.split("<br/>"));
			accum
		}) {
		let trimmed_line = line.trim();
		if trimmed_line.is_empty() {
			continue;
		}

		if let Some(result_code) = trimmed_line.strip_prefix("RESULT:") {
			returned_result_code = result_code;
			if result_code == "OK" {
				was_successful = true;
			}
		} else if trimmed_line.starts_with("INFO:")
			|| trimmed_line.starts_with("ERROR:")
			|| trimmed_line.starts_with("WARN:")
		{
			log_lines.push(trimmed_line);
		} else {
			extra_lines.push(trimmed_line);
		}
	}

	if !was_successful {
		warn!(
			log_lines = valuable(&log_lines),
			extra_lines = valuable(&extra_lines),
			%operation_name,
			result_code = %returned_result_code,
			"got an error from a result status page",
		);
	}

	Ok(was_successful)
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[test]
	pub fn can_parse_response_from_power_on_v2_on_14_80() {
		// Yes. This is a real HTML response, returned from a real MION running,
		// real, official firmware.
		//
		// Yes it is invalid HTML. Yes it is a mess. Yes it is not always returned
		// based off the parameters passed.
		assert!(
			parse_result_from_body(
				"<HTML>\r\nINFO: Enabling CATDEV mode...<br>\nINFO: no change in parameters<br>\nINFO: Enabling HSATA keep alive<br/>\nRESULT:OK<br>cafe powered on successfully, nAtt=0<br>\n</body>\n</HTML>",
				"power_on_v2"
			).expect("Failed to parse result from body!"),
			"Expected successful responsee."
		);
	}
}
