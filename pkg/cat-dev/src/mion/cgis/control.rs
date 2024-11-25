//! API's for interacting with `/mion/control.cgi`, an HTTP interface for
//! turning the device on & off.

use crate::{
	errors::{CatBridgeError, NetworkError, NetworkParseError},
	mion::{
		cgis::AUTHZ_HEADER,
		proto::cgis::{ControlOperation, SetParameter},
	},
};
use fnv::FnvHashMap;
use local_ip_address::local_ip;
use reqwest::{Client, Response, Version};
use serde::Serialize;
use std::net::Ipv4Addr;
use tracing::{field::valuable, warn};

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
	let body_result = response.bytes().await.map_err(NetworkError::ReqwestError);
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
	let response = do_raw_control_request(
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
	let status = response.status().as_u16();
	let body_result = response.bytes().await.map_err(NetworkError::ReqwestError);
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
/// This can also override some pre-existing configurations, e.g. if you send
/// a POST with emulation set to off even if PCFS is set to on, the cat-dev
/// will techincally still boot.
///
/// Power ON V2 is the power on API that the actual tools nintendo built use.
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
	atapi_port: Option<u16>,
	pcfs_port: Option<u16>,
	emulate_fs: bool,
) -> Result<bool, CatBridgeError> {
	power_on_v2_with_raw_client(
		&Client::default(),
		mion_ip,
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
/// Power ON V2 is the power on API that the actual tools nintendo built use.
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
	atapi_port: Option<u16>,
	pcfs_port: Option<u16>,
	emulate_fs: bool,
) -> Result<bool, CatBridgeError> {
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
		(
			"host",
			format!("{}", local_ip().map_err(NetworkError::LocalIpError)?),
		),
	];
	if let Some(port) = atapi_port {
		parameters.push(("atapi_port", format!("{port}")));
	}
	if let Some(port) = pcfs_port {
		parameters.push(("pcfs_port", format!("{port}")));
	}

	let response = do_raw_control_request(client, mion_ip, &parameters).await?;

	let status = response.status().as_u16();
	let body_result = response.bytes().await.map_err(NetworkError::ReqwestError);
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
) -> Result<Response, NetworkError>
where
	UrlEncodableType: Serialize,
{
	Ok(client
		.post(format!("http://{mion_ip}/mion/control.cgi"))
		.version(Version::HTTP_11)
		.header("authorization", format!("Basic {AUTHZ_HEADER}"))
		.header("content-type", "application/x-www-form-urlencoded")
		.header(
			"user-agent",
			format!("cat-dev/{}", env!("CARGO_PKG_VERSION")),
		)
		.body::<String>(
			serde_urlencoded::to_string(&url_parameters)
				.map_err(NetworkParseError::FormDataEncodeError)?,
		)
		.send()
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

fn parse_result_from_body(body: &str, operation_name: &str) -> Result<bool, CatBridgeError> {
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

	let mut was_successful = false;
	let mut returned_result_code = "";
	let mut log_lines = Vec::with_capacity(0);
	let mut extra_lines = Vec::with_capacity(0);
	for line in without_newlines
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
			"got an error back from mion/control.cgi",
		);
	}

	Ok(was_successful)
}
