//! API's for interacting with `/mion/control.cgi`, an HTTP interface for
//! turning the device on & off.

use crate::{
	errors::{APIError, CatBridgeError, NetworkError, NetworkParseError},
	mion::{
		cgis::{do_simple_request, parse_result_from_body},
		proto::cgis::{ControlOperation, SetParameter},
	},
};
use fnv::FnvHashMap;
use local_ip_address::local_ip;
use reqwest::{Client, Method};
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
pub async fn get_info_with_raw_client(
	client: &Client,
	mion_ip: Ipv4Addr,
	name: &str,
) -> Result<FnvHashMap<String, String>, CatBridgeError> {
	let body_as_string = do_raw_control_request(
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

	extract_body_tags(&body_as_string, ControlOperation::GetInfo.into())
}

/// Perform a `set_param` request given a host, and the parameter to set.
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn set_param(
	mion_ip: Ipv4Addr,
	parameter_to_set: SetParameter,
) -> Result<bool, CatBridgeError> {
	set_param_with_raw_client(&Client::default(), mion_ip, parameter_to_set).await
}

/// Set a parameter on the MION, but with an already existing HTTP Client.
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn set_param_with_raw_client(
	client: &Client,
	mion_ip: Ipv4Addr,
	parameter_to_set: SetParameter,
) -> Result<bool, CatBridgeError> {
	let body_as_string = do_raw_control_request(
		client,
		mion_ip,
		&[
			(
				"operation".to_owned(),
				Into::<&str>::into(ControlOperation::SetParam).to_owned(),
			),
			(
				format!("{parameter_to_set}"),
				parameter_to_set.get_value_as_string(),
			),
		],
	)
	.await?;

	parse_result_from_body(
		&body_as_string,
		Into::<&str>::into(ControlOperation::SetParam),
	)
}

/// Initiate a power-on request to turn on the CAT-DEV machine.
///
/// Note: This request just starts the actual power on, by the time you get to
/// powering on the device, if you are using things like emulation you should
/// already be connected to the SDIO ports, and be listening for ATAPI
/// requests.
///
/// This is an older power on API and ***you should always prefer
/// `power_on_v2` if it is possible to use.***
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn power_on(mion_ip: Ipv4Addr) -> Result<bool, CatBridgeError> {
	power_on_with_raw_client(&Client::default(), mion_ip).await
}

/// Initiate a power-on request to turn on the CAT-DEV machine.
///
/// Note: This request just starts the actual power on, by the time you get to
/// powering on the device, if you are using things like emulation you should
/// already be connected to the SDIO ports, and be listening for ATAPI
/// requests.
///
/// This is an older power on API and ***you should always prefer
/// `power_on_v2` if it is possible to use.***
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn power_on_with_raw_client(
	client: &Client,
	mion_ip: Ipv4Addr,
) -> Result<bool, CatBridgeError> {
	let body_as_string = do_raw_control_request(
		client,
		mion_ip,
		&[(
			"operation",
			Into::<&str>::into(ControlOperation::PowerOn).to_owned(),
		)],
	)
	.await?;
	parse_result_from_body(
		&body_as_string,
		Into::<&str>::into(ControlOperation::PowerOnV2),
	)
}

/// Initiate a power-on request to turn on the CAT-DEV machine.
///
/// Note: This request just starts the actual power on, by the time you get to
/// powering on the device, if you are using things like emulation you should
/// already be connected to the SDIO ports, and be listening for ATAPI
/// requests.
///
/// This can also override some pre-existing configurations, e.g. if you send
/// a POST with emulation set to off even if PCFS is set to on, the cat-dev
/// will techincally still boot.
///
/// ***Power ON V2 is only available on MIONs running at least 00.14.77 in
/// terms of firmware.***
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn power_on_v2(
	mion_ip: Ipv4Addr,
	host_ip: Option<Ipv4Addr>,
	atapi_port: Option<u16>,
	pcfs_port: Option<u16>,
	emulate_fs: bool,
) -> Result<bool, CatBridgeError> {
	power_on_v2_with_raw_client(
		&Client::default(),
		mion_ip,
		host_ip,
		atapi_port,
		pcfs_port,
		emulate_fs,
	)
	.await
}

/// Initiate a power-on request to turn on the CAT-DEV machine.
///
/// Note: This request just starts the actual power on, by the time you get to
/// powering on the device, if you are using things like emulation you should
/// already be connected to the SDIO ports, and be listening for ATAPI
/// requests.
///
/// This can also override some pre-existing configurations, e.g. if you send
/// a POST with emulation set to off even if PCFS is set to on, the cat-dev
/// will techincally still boot.
///
/// ***Power ON V2 is only available on MIONs running at least 00.14.77 in
/// terms of firmware.***
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn power_on_v2_with_raw_client(
	client: &Client,
	mion_ip: Ipv4Addr,
	host_ip: Option<Ipv4Addr>,
	atapi_port: Option<u16>,
	pcfs_port: Option<u16>,
	emulate_fs: bool,
) -> Result<bool, CatBridgeError> {
	let host_ip_as_str = if let Some(ip) = host_ip {
		format!("{ip}")
	} else {
		let ip = local_ip().map_err(|_| APIError::NoHostIpFound)?;
		format!("{ip}")
	};

	let mut parameters = vec![
		(
			"operation",
			Into::<&str>::into(ControlOperation::PowerOnV2).to_owned(),
		),
		(
			"emulation",
			if emulate_fs {
				"on".to_owned()
			} else {
				"off".to_owned()
			},
		),
		("host", host_ip_as_str),
	];
	if let Some(port) = atapi_port {
		parameters.push(("atapi_port", format!("{port}")));
	}
	if let Some(port) = pcfs_port {
		parameters.push(("pcfs_port", format!("{port}")));
	}

	let body_as_string = do_raw_control_request(client, mion_ip, &parameters).await?;
	parse_result_from_body(
		&body_as_string,
		Into::<&str>::into(ControlOperation::PowerOnV2),
	)
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
pub async fn do_raw_control_request<'key, 'value, UrlEncodableType>(
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
		format!("http://{mion_ip}/mion/control.cgi"),
		Some(
			serde_urlencoded::to_string(&url_parameters)
				.map_err(NetworkParseError::FormDataEncodeError)
				.map_err(NetworkError::ParseError)?,
		),
	)
	.await
}

/// Extract tags from body request.
///
/// "tags" are values separated by `<br>`, and separated by `:`.
///
/// ## Errors
///
/// - If we cannot find the start `<body>` tag.
/// - If we cannot find the end `</body>` tag.
fn extract_body_tags(
	body: &str,
	operation_name: &str,
) -> Result<FnvHashMap<String, String>, CatBridgeError> {
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
				warn!(%operation_name, "Unparsable line from body on mion/control.cgi: {line}");
				None
			}
		})
		.collect::<FnvHashMap<String, String>>();

	Ok(fields)
}
