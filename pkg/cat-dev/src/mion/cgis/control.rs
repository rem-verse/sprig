//! API's for interacting with `/mion/control.cgi`, an HTTP interface for
//! turning the device on & off.

use crate::{
	errors::{CatBridgeError, NetworkError, NetworkParseError},
	mion::{cgis::AUTHZ_HEADER, proto::cgis::ControlOperation},
};
use fnv::FnvHashMap;
use hyper::{
	body::to_bytes as read_http_body_bytes,
	client::{connect::Connect, Client},
	Body, Request, Response, Version,
};
use local_ip_address::local_ip;
use serde::Serialize;
use std::net::Ipv4Addr;
use tracing::warn;

/// Perform a `get_info` request given a host, and a name.
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn get_info(
	mion_ip: Ipv4Addr,
	name: &str,
) -> Result<FnvHashMap<String, String>, CatBridgeError> {
	get_info_with_raw_client(&Client::default(), mion_ip, name).await
}

/// Perform a get info request, but with an already existing HTTP client.
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn get_info_with_raw_client<ClientConnectorTy>(
	client: &Client<ClientConnectorTy>,
	mion_ip: Ipv4Addr,
	name: &str,
) -> Result<FnvHashMap<String, String>, CatBridgeError>
where
	ClientConnectorTy: Clone + Connect + Send + Sync + 'static,
{
	let response = do_raw_control_request(
		client,
		mion_ip,
		&[
			("operation", Into::<&str>::into(ControlOperation::GetInfo)),
			(
				"host",
				&format!("{}", local_ip().map_err(NetworkError::LocalIpError)?),
			),
			("shutdown", "1"),
			("name", name),
		],
	)
	.await?;
	let status = response.status().as_u16();
	let body_result = read_http_body_bytes(response.into_body())
		.await
		.map_err(NetworkError::HyperError);
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
	let read_body_bytes = body_result?;
	let body_as_string = String::from_utf8(read_body_bytes.into())
		.map_err(NetworkParseError::InvalidDataNeedsUTF8)
		.map_err(NetworkError::ParseError)?;

	extract_body_tags(&body_as_string)
}

/// Perform a raw operation on the MION board's `control.cgi` page.
///
/// *note: you probably want to call one of the actual methods, as this is
/// basically just a thin wrapper around an HTTP Post Request. Not doing much
/// else more. A lot of it requires that you set things up correctly.*
///
/// ## Errors
///
/// - If we cannot make an HTTP request to the MION Request.
/// - If we fail to encode your parameters into a request body.
pub async fn do_raw_control_request<'key, 'value, ClientConnectorTy, UrlEncodableType>(
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
			Request::post(format!("http://{mion_ip}/mion/control.cgi"))
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

/// Extract tags from body request.
///
/// "tags" are values separated by `<br>`, and separated by `:`.
///
/// ## Errors
///
/// - If we cannot find the start `<body>` tag.
/// - If we cannot find the end `</body>` tag.
fn extract_body_tags(body: &str) -> Result<FnvHashMap<String, String>, CatBridgeError> {
	let start_tag_location = body.find("<body>").map(|num| num + 6).ok_or_else(|| {
		CatBridgeError::NetworkError(NetworkError::ParseError(
			NetworkParseError::HtmlResponseMissingBody(body.to_owned()),
		))
	})?;
	let body_without_start_tag = body.split_at(start_tag_location).1;
	let end_tag_location = body_without_start_tag.find("</body>").ok_or_else(|| {
		CatBridgeError::NetworkError(NetworkError::ParseError(
			NetworkParseError::HtmlResponseMissingBody(body.to_owned()),
		))
	})?;
	let just_inner_body = body_without_start_tag.split_at(end_tag_location).0;

	let without_newlines = just_inner_body.replace('\n', "");
	let fields = without_newlines
		.split("<br>")
		.filter_map(|line| {
			// Remove all empty lines, and all log lines.
			if line.is_empty()
				|| line.trim().is_empty()
				|| line.starts_with("INFO:")
				|| line.starts_with("WARN:")
				|| line.starts_with("ERROR:")
			{
				None
			} else if let Some(location) = line.find(':') {
				let (key, value) = line.split_at(location);
				Some((key.to_owned(), value.trim_start_matches(':').to_owned()))
			} else {
				warn!("Unparsable line from body on control.cgi: {line}");
				None
			}
		})
		.collect::<FnvHashMap<String, String>>();

	Ok(fields)
}
