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
mod setup;
mod signal_get;

pub use control::*;
pub use dump_eeprom::*;
pub use dump_memory::*;
pub use setup::*;
pub use signal_get::*;

use crate::errors::{CatBridgeError, NetworkError, NetworkParseError};
use bytes::Bytes;
use reqwest::{Body, Client, Method, Response, Version};

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
) -> Result<String, CatBridgeError>
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
		assert_status_and_read_body(200, req.send().await.map_err(NetworkError::ReqwestError)?)
			.await?;

	String::from_utf8(response_body.into())
		.map_err(NetworkParseError::InvalidDataNeedsUTF8)
		.map_err(NetworkError::ParseError)
		.map_err(CatBridgeError::NetworkError)
}

/// Assert that a response status code is a specific code, and get the body.
///
/// ## Errors
///
/// If the status code did not match the expected result.
async fn assert_status_and_read_body(
	needed_status: u16,
	response: Response,
) -> Result<Bytes, CatBridgeError> {
	let status = response.status().as_u16();
	let body_result = response.bytes().await.map_err(NetworkError::ReqwestError);
	if status != needed_status {
		if let Ok(body) = body_result {
			return Err(CatBridgeError::NetworkError(NetworkError::ParseError(
				NetworkParseError::UnexpectedStatusCode(status, body),
			)));
		}

		return Err(CatBridgeError::NetworkError(NetworkError::ParseError(
			NetworkParseError::UnexpectedStatusCodeNoBody(status),
		)));
	}

	Ok(body_result?)
}
