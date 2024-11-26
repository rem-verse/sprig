//! API's for interacting with `/mion/status.cgi`.
//!
//! These APIs allow changing the status of Disc In/Disc Out, etc. You can view
//! the setup page on the actual main page of the MION dashboard.

use crate::{
	errors::{CatBridgeError, NetworkError, NetworkParseError},
	mion::{
		cgis::{do_simple_request, parse_result_from_body},
		proto::cgis::StatusOperation,
	},
};
use reqwest::{Client, Method};
use serde::Serialize;
use std::net::Ipv4Addr;

/// Perform an `eject` event to set the current DISC IN/Eject state for the
/// MION.
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn set_disc_eject_state(
	mion_ip: Ipv4Addr,
	disc_in: bool,
) -> Result<bool, CatBridgeError> {
	set_disc_eject_state_with_raw_client(&Client::default(), mion_ip, disc_in).await
}

/// Perform an `eject` event to set the current DISC IN/Eject state for the
/// MION, but with an already existing HTTP Client.
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn set_disc_eject_state_with_raw_client(
	client: &Client,
	mion_ip: Ipv4Addr,
	disc_in: bool,
) -> Result<bool, CatBridgeError> {
	let body_as_string = do_raw_status_request(
		client,
		mion_ip,
		&[
			("operation", Into::<&str>::into(StatusOperation::Eject)),
			("disc", if disc_in { "in" } else { "out" }),
			("frommenu", "no"),
		],
	)
	.await?;

	parse_result_from_body(&body_as_string, Into::<&str>::into(StatusOperation::Eject))
}

/// Perform a raw operation on the MION board's `/mion/status.cgi` page.
///
/// *note: you probably want to call one of the actual methods, as this is
/// basically just a thin wrapper around an HTTP Post Request. Not doing much
/// else more. A lot of it requires that you set things up correctly.*
///
/// ## Errors
///
/// - If we cannot make an HTTP request to the MION Request.
/// - If we fail to encode your parameters into a request body.
pub async fn do_raw_status_request<'key, 'value, UrlEncodableType>(
	client: &Client,
	mion_ip: Ipv4Addr,
	url_parameters: UrlEncodableType,
) -> Result<String, CatBridgeError>
where
	UrlEncodableType: Serialize,
{
	do_simple_request::<String>(
		client,
		Method::POST,
		format!("http://{mion_ip}/mion/status.cgi"),
		Some(
			serde_urlencoded::to_string(&url_parameters)
				.map_err(NetworkParseError::FormDataEncodeError)
				.map_err(NetworkError::ParseError)?,
		),
	)
	.await
}
