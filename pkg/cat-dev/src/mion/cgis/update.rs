//! API's for interacting with `/update.cgi`.
//!
//! This update page is the "entry-point" for dealing with FW versions, and
//! updates of the underlying MION board itself. Other CGIs implement the
//! actual "yes commit I want to update to this version".
//!
//! *note: right now only support for getting versions is implemented, as
//! mentioned the upload flow travels through significantly more pages, and has
//! more buggy behavior I have chosen to not implement yet. PRs accepted.*

use crate::{
	errors::NetworkError,
	mion::{
		cgis::do_simple_request,
		proto::cgis::{MIONCGIErrors, MIONFirmwareVersions},
	},
};
use reqwest::{Body, Client, Method};
use std::net::Ipv4Addr;

/// The start of the upload forms should always begin with this constant value.
const FORM_PREFIX: &str = r#"<form method="POST" enctype="multipart/form-data""#;
/// The end of an HTML Form, for extracting without.
const FORM_SUFFIX: &str = "</form>";
/// The version string should always print this static string before printing
/// the version.
const VERISON_PREFIX: &str = "Version : ";

/// Perform a get request to `update.cgi` to fetch the current versions.
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn get_versions(mion_ip: Ipv4Addr) -> Result<MIONFirmwareVersions, NetworkError> {
	get_versions_with_raw_client(&Client::default(), mion_ip).await
}

/// Perform a get request to `update.cgi` to fetch the current versions, but
/// with an already existing HTTP client.
///
/// ## Errors
///
/// - If we cannot encode the parameters as a form url encoded.
/// - If we cannot make the HTTP request.
/// - If the server does not respond with a 200.
/// - If we cannot read the body from HTTP.
/// - If we cannot parse the HTML response.
pub async fn get_versions_with_raw_client(
	client: &Client,
	mion_ip: Ipv4Addr,
) -> Result<MIONFirmwareVersions, NetworkError> {
	let body_as_string = do_simple_request::<Body>(
		client,
		Method::GET,
		format!("http://{mion_ip}/update.cgi"),
		None,
	)
	.await?;

	let (fw_version, fpga_version) = parse_versions_from_update_html(&body_as_string)?;
	Ok(MIONFirmwareVersions::from_versions(
		fw_version,
		fpga_version,
	))
}

fn parse_versions_from_update_html(body_as_string: &str) -> Result<([u8; 3], u32), MIONCGIErrors> {
	// Parse out the `<body>` tag, we can't close the actual body tag because
	// _sometimes_ the MION will add in custom colors.
	let start_tag_location = body_as_string
		.find("<body")
		.map(|num| num + 5)
		.ok_or_else(|| MIONCGIErrors::HtmlResponseMissingBody(body_as_string.to_owned()))?;
	let body_without_start_tag = body_as_string.split_at(start_tag_location).1;
	let end_tag_location = body_without_start_tag
		.find("</body>")
		.ok_or_else(|| MIONCGIErrors::HtmlResponseMissingBody(body_as_string.to_owned()))?;
	let body_contents = body_without_start_tag
		.split_at(end_tag_location)
		.0
		.to_owned();
	let versions = get_version_elements(&body_contents)?;
	// We should get an FPGA version, and a FW version. IPL versions are tracked
	// elsewhere.
	if versions.len() != 2 {
		return Err(MIONCGIErrors::HtmlResponseMissingVersions(versions));
	}

	let fpga_version = u32::from_str_radix(&versions[0], 16)
		.map_err(MIONCGIErrors::HtmlResponseNumberExpectedButNotThere)?;
	let mut fw_version = [0_u8; 3];
	for (idx, item) in versions[1].splitn(3, '.').enumerate() {
		fw_version[idx] = item
			.parse::<u8>()
			.map_err(MIONCGIErrors::HtmlResponseNumberExpectedButNotThere)?;
	}

	Ok((fw_version, fpga_version))
}

/// Parse all of the version strings out of an HTML response.
fn get_version_elements(mut html_response: &str) -> Result<Vec<String>, MIONCGIErrors> {
	let mut results = Vec::new();

	while let Some(idx) = html_response.find(FORM_PREFIX) {
		// This techincally doesn't cut off the ending of the `<form>` tag! This is
		// expected because certain firmwares in certain states can choose to add
		// in color fields, and sometimes not.
		//
		// Prettyness gets in the way of our "parsing", if you can even call it
		// that.
		let missing_form_start = html_response.split_at(idx + FORM_PREFIX.len()).1;
		let Some(form_close_at) = missing_form_start.find(FORM_SUFFIX) else {
			return Err(MIONCGIErrors::HtmlResponseMissingClosingTag(
				FORM_SUFFIX.to_owned(),
				missing_form_start.to_owned(),
			));
		};
		let (form_insides, form_outsides) = missing_form_start.split_at(form_close_at);

		// Skip past the `</form>` we just found for future loops.
		html_response = &form_outsides[FORM_SUFFIX.len()..];

		// Remove any HTML elements from the form, we only care about the text
		// at the beginning.
		let version_data = if let Some(loc) = form_insides.find("<input") {
			form_insides.split_at(loc).0.trim().replace("<br>", "")
		} else {
			form_insides.trim().replace("<br>", "")
		};
		let Some(version_prefix_location) = version_data.find(VERISON_PREFIX) else {
			return Err(MIONCGIErrors::HtmlResponseMissingVersionPart(
				VERISON_PREFIX.to_owned(),
				version_data,
			));
		};

		let version_with_extra_data = version_data
			.split_at(version_prefix_location + VERISON_PREFIX.len())
			.1;
		let Some(version_suffix_location) = version_with_extra_data.find(')') else {
			return Err(MIONCGIErrors::HtmlResponseMissingVersionPart(
				")".to_owned(),
				version_with_extra_data.to_owned(),
			));
		};
		results.push(
			version_with_extra_data
				.split_at(version_suffix_location)
				.0
				.trim()
				.to_owned(),
		);
	}

	Ok(results)
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	/// The response for a real life update page, minus some HTML color stringt hashtags
	/// to make the string easier to handle in Rust.
	const REAL_LIFE_UPDATE_PAGE: &str = r#"<HTML>
<head>
<meta http-equiv="Content-Type" content="text/html; charset=ASCII">
<meta http-equiv="Pragma" content="no-cache"><meta http-equiv="Cache-Control" content="no-cache"><meta http-equiv="Expires" content="0"><title>Program Update</title>
<script type="text/javascript" src="pwiss.js"></script>
</head>
<body bgcolor="FFFFFF" text="000000">
<div align="center">
<h1>Program Update</h1><br><br>
<div id="disp1">
<table border=0 cellspacing=0 cellpadding=0>
<tr>
<td>
<form method="POST" enctype="multipart/form-data" action="update_fpga.cgi">
FPGA Data&nbsp;(Present FPGA Version : 13052071)<br>
<input type="hidden" name="func" value="fpga_upd">
<input type="file" name="filename" size=50>
<input type="submit" value="Upload" OnClick="disp_change();">
<br>
</form>
<form method="POST" enctype="multipart/form-data" action="update_fw.cgi">
Firmware&nbsp;(Present Firmware Version : 00.14.70)<br>
<input type="hidden" name="func" value="fw_upd">
<input type="file" name="filename" size=50>
<input type="submit" value="Upload" OnClick="disp_change();">
<br>
</form>
</td>
</tr>
</table>
</div>
<div id="disp2" style="display:none;">
  File Uploading . . .<br>
<br>
</div>
</div>
<hr>
<a href="./">Homepage</a>
</body>
</HTML>"#;

	#[test]
	pub fn can_parse_update_html() {
		let (fw_version, fpga_verison) = parse_versions_from_update_html(REAL_LIFE_UPDATE_PAGE)
			.expect("Failed to parse versions out of a 0.0.14.70 update.cgi page!");
		assert_eq!(
			fw_version,
			[0_u8, 14_u8, 70_u8],
			"Firmware Version did not match expected!"
		);
		assert_eq!(
			fpga_verison, 0x13052071,
			"FPGA version did not match expected!",
		);
	}
}
