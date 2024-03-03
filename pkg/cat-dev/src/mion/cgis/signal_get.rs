//! API's for interacting with `/signal_get.cgi`, an HTTP interface for
//! getting signals

use crate::{
	errors::{CatBridgeError, NetworkError, NetworkParseError},
	mion::cgis::AUTHZ_HEADER,
};
use hyper::{
	body::to_bytes as read_http_body_bytes,
	client::{connect::Connect, Client},
	Body, Request, Response, Version,
};
use serde::Serialize;
use std::net::Ipv4Addr;

/// Perform a `signal_get` request for the `VDD2` signal given a host.
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn get_vdd2(mion_ip: Ipv4Addr) -> Result<String, CatBridgeError> {
	get_vdd2_with_raw_client(&Client::default(), mion_ip).await
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
pub async fn get_vdd2_with_raw_client<ClientConnectorTy>(
	client: &Client<ClientConnectorTy>,
	mion_ip: Ipv4Addr,
) -> Result<String, CatBridgeError>
where
	ClientConnectorTy: Clone + Connect + Send + Sync + 'static,
{
	let response = do_raw_signal_http_request(client, mion_ip, &[("sig", "VDD2")]).await?;
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

	let start_tag_location = body_as_string
		.find("<body>")
		.map(|num| num + 6)
		.ok_or_else(|| {
			CatBridgeError::NetworkError(NetworkError::ParseError(
				NetworkParseError::HtmlResponseMissingBody(body_as_string.clone()),
			))
		})?;
	let body_without_start_tag = body_as_string.split_at(start_tag_location).1;
	let end_tag_location = body_without_start_tag.find("</body>").ok_or_else(|| {
		CatBridgeError::NetworkError(NetworkError::ParseError(
			NetworkParseError::HtmlResponseMissingBody(body_as_string.clone()),
		))
	})?;

	Ok(body_without_start_tag
		.split_at(end_tag_location)
		.0
		.to_owned())
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
pub async fn do_raw_signal_http_request<'key, 'value, ClientConnectorTy, UrlEncodableType>(
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
			Request::post(format!("http://{mion_ip}/signal_get.cgi"))
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
