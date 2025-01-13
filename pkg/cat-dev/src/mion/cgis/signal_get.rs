//! API's for interacting with `/signal_get.cgi`, an HTTP interface for
//! getting signals

use crate::{
	errors::NetworkError,
	mion::{cgis::do_simple_request, proto::cgis::MIONCGIErrors},
};
use reqwest::{Client, Method};
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
pub async fn get_vdd2(mion_ip: Ipv4Addr) -> Result<String, NetworkError> {
	get_vdd2_with_raw_client(&Client::default(), mion_ip).await
}

/// Perform a `signal_get` request for the `VDD2` signal given a host,
/// but with an already existing HTTP client.
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn get_vdd2_with_raw_client(
	client: &Client,
	mion_ip: Ipv4Addr,
) -> Result<String, NetworkError> {
	let body_as_string = do_raw_signal_http_request(client, mion_ip, &[("sig", "VDD2")]).await?;

	let start_tag_location = body_as_string
		.find("<body>")
		.map(|num| num + 6)
		.ok_or_else(|| MIONCGIErrors::HtmlResponseMissingBody(body_as_string.clone()))?;
	let body_without_start_tag = body_as_string.split_at(start_tag_location).1;
	let end_tag_location = body_without_start_tag
		.find("</body>")
		.ok_or_else(|| MIONCGIErrors::HtmlResponseMissingBody(body_as_string.clone()))?;

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
pub async fn do_raw_signal_http_request<UrlEncodableType>(
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
		format!("http://{mion_ip}/signal_get.cgi"),
		Some(
			serde_urlencoded::to_string(&url_parameters)
				.map_err(MIONCGIErrors::FormDataEncodeError)?,
		),
	)
	.await
}
