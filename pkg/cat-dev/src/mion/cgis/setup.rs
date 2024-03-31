//! APIs for parsing the data from `/setup.cgi`, an HTTP interface usually
//! only meant for humans.

use crate::{
	errors::{CatBridgeError, NetworkError, NetworkParseError},
	mion::{cgis::do_simple_request, proto::cgis::SetupParameters},
};
use mac_address::MacAddress;
use reqwest::{Body, Client, Method};
use std::net::Ipv4Addr;

/// Get the setup parameters from a particular MION IP.
///
/// ## Errors
///
/// If we cannot make a network request to the `setup.cgi` page, or get some
/// sort of error condition back.
pub async fn get_setup_parameters(mion_ip: Ipv4Addr) -> Result<SetupParameters, CatBridgeError> {
	get_setup_parameters_with_raw_client(&Client::default(), mion_ip).await
}

/// Get the setup parameters from a particular MION IP.
///
/// ## Errors
///
/// If we cannot make a network request to the `setup.cgi` page, or get some
/// sort of error condition back.
pub async fn get_setup_parameters_with_raw_client(
	client: &Client,
	mion_ip: Ipv4Addr,
) -> Result<SetupParameters, CatBridgeError> {
	let body_as_string = do_simple_request::<Body>(
		client,
		Method::GET,
		format!("http://{mion_ip}/mion/control.cgi"),
		None,
	)
	.await?;

	parse_response_body_from_setup(&body_as_string)
}

/// Parse the actual setup parameters from an HTML body.
///
/// This is seperated into it's own method for testability.
///
/// ## Errors
///
/// - If we can't parse the response as the HTML format MIONs should respond
///   with.
/// - If the type of inputs were incorrect.
fn parse_response_body_from_setup(body_as_string: &str) -> Result<SetupParameters, CatBridgeError> {
	Ok(SetupParameters::new(
		find_input_from_body(&InputType::Text, "id_5", body_as_string)?
			.parse::<Ipv4Addr>()
			.map_err(NetworkParseError::HtmlResponseIpExpectedButNotThere)
			.map_err(NetworkError::ParseError)?,
		find_input_from_body(&InputType::Text, "id_6", body_as_string)?
			.parse::<Ipv4Addr>()
			.map_err(NetworkParseError::HtmlResponseIpExpectedButNotThere)
			.map_err(NetworkError::ParseError)?,
		find_input_from_body(&InputType::Text, "id_7", body_as_string)?
			.parse::<Ipv4Addr>()
			.map_err(NetworkParseError::HtmlResponseIpExpectedButNotThere)
			.map_err(NetworkError::ParseError)?,
		find_input_from_body(&InputType::BooleanRadio, "id_4", body_as_string)?
			.parse::<bool>()
			.expect("impossible, this function only returns Ok(true) or Ok(false)"),
		find_input_from_body(&InputType::BooleanRadio, "id_8", body_as_string)?
			.parse::<bool>()
			.expect("impossible, this function only returns Ok(true) or Ok(false)"),
		find_input_from_body(&InputType::Text, "id_9", body_as_string)?
			.parse::<Ipv4Addr>()
			.map_err(NetworkParseError::HtmlResponseIpExpectedButNotThere)
			.map_err(NetworkError::ParseError)?,
		find_input_from_body(&InputType::Text, "id_10", body_as_string)?
			.parse::<Ipv4Addr>()
			.map_err(NetworkParseError::HtmlResponseIpExpectedButNotThere)
			.map_err(NetworkError::ParseError)?,
		find_input_from_body(&InputType::BooleanRadio, "id_11", body_as_string)?
			.parse::<bool>()
			.expect("impossible, this function only returns Ok(true) or Ok(false)"),
		find_input_from_body(&InputType::Text, "id_12", body_as_string)?
			.parse::<Ipv4Addr>()
			.map_err(NetworkParseError::HtmlResponseIpExpectedButNotThere)
			.map_err(NetworkError::ParseError)?,
		find_input_from_body(&InputType::SelectedResultValue, "id_26", body_as_string)?
			.parse::<u32>()
			.map_err(NetworkParseError::HtmlResponseNumberExpectedButNotThere)
			.map_err(NetworkError::ParseError)?
			.try_into()?,
		find_input_from_body(&InputType::Text, "id_27", body_as_string)?
			.parse::<u8>()
			.map_err(NetworkParseError::HtmlResponseNumberExpectedButNotThere)
			.map_err(NetworkError::ParseError)?,
		find_input_from_body(&InputType::Text, "id_13", body_as_string)?
			.parse::<u16>()
			.map_err(NetworkParseError::HtmlResponseNumberExpectedButNotThere)
			.map_err(NetworkError::ParseError)?,
		find_input_from_body(&InputType::Text, "id_14", body_as_string)?
			.parse::<u16>()
			.map_err(NetworkParseError::HtmlResponseNumberExpectedButNotThere)
			.map_err(NetworkError::ParseError)?,
		find_input_from_body(&InputType::Text, "id_15", body_as_string)?
			.parse::<u16>()
			.map_err(NetworkParseError::HtmlResponseNumberExpectedButNotThere)
			.map_err(NetworkError::ParseError)?,
		find_input_from_body(&InputType::Text, "id_16", body_as_string)?
			.parse::<u16>()
			.map_err(NetworkParseError::HtmlResponseNumberExpectedButNotThere)
			.map_err(NetworkError::ParseError)?,
		find_input_from_body(&InputType::Text, "id_17", body_as_string)?
			.parse::<u16>()
			.map_err(NetworkParseError::HtmlResponseNumberExpectedButNotThere)
			.map_err(NetworkError::ParseError)?,
		find_input_from_body(&InputType::BooleanRadio, "id_33", body_as_string)?
			.parse::<bool>()
			.expect("impossible, this function only returns Ok(true) or Ok(false)"),
		find_input_from_body(&InputType::BooleanRadio, "id_32", body_as_string)?
			.parse::<bool>()
			.expect("impossible, this function only returns Ok(true) or Ok(false)"),
		find_input_from_body(&InputType::Text, "id_28", body_as_string)?,
		find_input_from_body(&InputType::Text, "id_29", body_as_string)?,
		find_input_from_body(&InputType::Text, "id_30", body_as_string)?,
		find_input_from_body(&InputType::Text, "id_31", body_as_string)?,
		find_input_from_body(&InputType::TextArea, "id_2", body_as_string)?,
		find_input_from_body(
			&InputType::TableItemWithPrefix("MAC Address"),
			"",
			body_as_string,
		)?
		.parse::<MacAddress>()
		.map_err(NetworkParseError::HtmlResponseMacExpectedButNotThere)
		.map_err(NetworkError::ParseError)?,
	))
}

enum InputType<'inner> {
	Text,
	TextArea,
	BooleanRadio,
	SelectedResultValue,
	TableItemWithPrefix(&'inner str),
}

fn find_input_from_body(
	input_typ: &InputType<'_>,
	input_id: &str,
	html_response: &str,
) -> Result<String, NetworkError> {
	match input_typ {
		InputType::Text => {
			let string_to_find = format!("<input type=\"text\" name=\"{input_id}\"");
			let pos = get_html_value(input_id, html_response, html_response, &string_to_find)?;

			let starts_at_input = &html_response[pos..];
			let value_start = get_html_value(input_id, html_response, starts_at_input, "value=\"")?;
			let next_value = &starts_at_input[value_start + 7..];
			let ending_value = get_html_value(input_id, html_response, next_value, "\">")?;
			let final_value = &next_value[..ending_value];

			Ok(final_value.to_owned())
		}
		InputType::TextArea => {
			let pos = get_html_value(
				input_id,
				html_response,
				html_response,
				&format!("<textarea name=\"{input_id}\""),
			)?;
			let starts_at_input = &html_response[pos..];
			let end_tag_pos = get_html_value(input_id, html_response, starts_at_input, ">")?;
			let minus_start_tag = &starts_at_input[end_tag_pos + 1..];
			let closing_tag =
				get_html_value(input_id, html_response, minus_start_tag, "</textarea>")?;
			let value = &minus_start_tag[..closing_tag];

			Ok(value.to_owned())
		}
		InputType::BooleanRadio => {
			if html_response.contains(&format!(
				"<input type=\"radio\" name=\"{input_id}\" value=\"1\" checked>"
			)) {
				Ok("true".to_owned())
			} else if html_response.contains(&format!(
				"<input type=\"radio\" name=\"{input_id}\" value=\"0\" checked>"
			)) {
				Ok("false".to_owned())
			} else {
				Err(NetworkError::ParseError(
					NetworkParseError::HtmlResponseNoRadioChecked(html_response.to_owned()),
				))
			}
		}
		InputType::SelectedResultValue => {
			let start_input_pos = get_html_value(
				input_id,
				html_response,
				html_response,
				&format!("<select name=\"{input_id}\">"),
			)?;
			let start_at_select_value = &html_response[start_input_pos + 15 + input_id.len()..];
			let select_end_pos =
				get_html_value(input_id, html_response, start_at_select_value, "</select>")?;
			let selects = &start_at_select_value[..select_end_pos];
			let end_input_pos = get_html_value(input_id, html_response, selects, "\" selected>")?;
			let ends_at_selected_value = &selects[..end_input_pos];
			let start_option_location = rget_html_value(
				input_id,
				html_response,
				ends_at_selected_value,
				"<option value=\"",
			)?;

			let value_as_string = &ends_at_selected_value[start_option_location + 15..];
			Ok(value_as_string.to_owned())
		}
		InputType::TableItemWithPrefix(table_prefix) => {
			for table_item in get_all_table_items(html_response) {
				let stripped_beginning = table_item
					.replace("&nbsp;", "")
					.replace("<br>", "")
					.replace("<br/>", "")
					.replace("<br />", "");
				let trimmed = stripped_beginning.trim();
				if !trimmed.starts_with(table_prefix) {
					continue;
				}
				let value = trimmed[table_prefix.len()..].trim();

				return Ok(value.to_owned());
			}

			Err(NetworkError::ParseError(
				NetworkParseError::HtmlResponseNoTableItemWithPrefix(
					html_response.to_owned(),
					table_prefix.to_owned().to_owned(),
				),
			))
		}
	}
}

fn get_all_table_items(body: &str) -> Vec<&str> {
	let mut table_items = Vec::new();

	let mut response = body;
	loop {
		let Some(next_table_start) = response.find("<td") else {
			break;
		};
		let minus_start_of_td = &response[next_table_start + 3..];
		let Some(tag_end) = minus_start_of_td.find('>') else {
			break;
		};
		let minus_opening_tag = &minus_start_of_td[tag_end + 1..];

		let Some(end_tag_start) = minus_opening_tag.find("</td>") else {
			break;
		};
		let minus_ending_tag = &minus_opening_tag[..end_tag_start];
		table_items.push(minus_ending_tag);

		response = &response[end_tag_start + 5..];
	}

	table_items
}

fn get_html_value(
	input_id: &str,
	html_response: &str,
	haystack: &str,
	needle: &str,
) -> Result<usize, NetworkError> {
	haystack.find(needle).ok_or_else(|| {
		NetworkError::ParseError(NetworkParseError::HtmlResponseMissingTaggedInput(
			input_id.to_owned(),
			html_response.to_owned(),
		))
	})
}
fn rget_html_value(
	input_id: &str,
	html_response: &str,
	haystack: &str,
	needle: &str,
) -> Result<usize, NetworkError> {
	haystack.rfind(needle).ok_or_else(|| {
		NetworkError::ParseError(NetworkParseError::HtmlResponseMissingTaggedInput(
			input_id.to_owned(),
			html_response.to_owned(),
		))
	})
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use crate::mion::proto::cgis::CatDevBankSize;

	/// A real world response to the MION setup page.
	const REAL_WORLD_SETUP_RESPONSE: &str = r##"<HTML>
<head>
<meta http-equiv="Content-Type" content="text/html; charset=ASCII">
<title>MION Board Parameter Settings</title>
</head>
<body bgcolor="#FFFFFF" text="#000000">
<div align="center">
<h1><u>MION Board Parameter Settings</u></h1>
<form method="POST" name="param_form" action="setup.cgi">
<table border=1 cellspacing=1 cellpadding=8>
 <tr>
  <td rowspan=9><font size="+1">Network</font></td>
  <td>IP Address<br><input type="text" name="id_5" maxlength=15 size=25 value="192.168.0.1"></td>
 </tr>
 <tr>
  <td>Subnet Mask<br><input type="text" name="id_6" maxlength=15 size=25 value="255.255.255.0"></td>
 </tr>
 <tr>
  <td>Default Gateway<br><input type="text" name="id_7" maxlength=15 size=25 value="0.0.0.0"></td>
 </tr>
 <tr>
  <td>
   DHCP<br>
   <input type="radio" name="id_4" value="1" checked>ON
   <input type="radio" name="id_4" value="0">OFF
  </td>
 </tr>
 <tr>
  <td>
   DNS<br>
   <input type="radio" name="id_8" value="1">ON
   <input type="radio" name="id_8" value="0" checked>OFF
  </td>
 </tr>
 <tr>
  <td>Preferred DNS Server Address<br><input type="text" name="id_9" maxlength=15 size=25 value="0.0.0.0"></td>
 </tr>
 <tr>
  <td>Alternate DNS Server Address<br><input type="text" name="id_10" maxlength=15 size=25 value="0.0.0.0"></td>
 </tr>
 <tr>
  <td>
   Jumbo Frame<br>
   <input type="radio" name="id_11" value="1" checked>ON
   <input type="radio" name="id_11" value="0">OFF
  </td>
 </tr>
 <tr>
  <td>
   Host PC IP Address<br>
   <input type="text" name="id_12" maxlength=15 size=25 value="0.0.0.0">
  </td>
 </tr>
 <tr>
  <td rowspan=2><font size="+1">SATA</font></td>
  <td>
   Internal HDD Bank Size<br>
   <select name="id_26">
    <option value="4294967295">&nbsp;
    <option value="0" selected>25GB
    <option value="1">5GB
    <option value="2">9GB
    <option value="3">12GB
    <option value="4">14GB
    <option value="5">16GB
    <option value="6">18GB
    <option value="7">21GB
   </select>
  </td>
 </tr>
 <tr>
  <td>HDD Bank No.<br><input type="text" name="id_27" size=25 value="0"></td>
 </tr>
 <tr>
  <td><font size="+1">ATAPI Emulator</font></td>
  <td>Port No. for ATAPI Emulator<br><input type="text" name="id_13" maxlength=5 size=25 value="7974"></td>
 </tr>
 <tr>
  <td><font size="+1">SDIO Printf/Control</font></td>
  <td>Port No. for SDIO Printf/Control<br><input type="text" name="id_14" maxlength=5 size=25 value="7975"></td>
 </tr>
 <tr>
  <td><font size="+1">SDIO Block Data</font></td>
  <td>Port No. for SDIO Block Data<br><input type="text" name="id_15" maxlength=5 size=25 value="7976"></td>
 </tr>
 <tr>
  <td><font size="+1">EXI</font></td>
  <td>Port No. for EXI<br><input type="text" name="id_16" maxlength=5 size=25 value="7977"></td>
 </tr>
 <tr>
  <td><font size="+1">Parameter Space</font></td>
  <td>Port No. for Parameter Space<br><input type="text" name="id_17" maxlength=5 size=25 value="7978"></td>
 </tr>
 <tr>
  <td><font size="+1">Drive Setting</font></td>
  <td>
   Drive Timing Emulation<br>
   <input type="radio" name="id_33" value="1">enable
   <input type="radio" name="id_33" value="0" checked>disable
  </td>
 </tr>
 <tr>
  <td><font size="+1">Operational Mode</font></td>
  <td>
   Operational Mode<br>
   <input type="radio" name="id_32" value="0" checked>CAT-DEV Mode
   <input type="radio" name="id_32" value="1">H-Reader Mode
  </td>
 </tr>
 <tr>
  <td rowspan=4><font size="+1">Drive Information</font></td>
  <td>Product Revision<br><input type="text" name="id_28" maxlength=4 size=25 value="0000">(HEX)</td>
 </tr>
 <tr>
  <td>Vendor Code<br><input type="text" name="id_29" maxlength=2 size=25 value="02">(HEX)</td>
 </tr>
 <tr>
  <td>Device Code<br><input type="text" name="id_30" maxlength=2 size=25 value="06">(HEX)</td>
 </tr>
 <tr>
  <td>   Release Date<br><input type="text" name="id_31" maxlength=8 size=25 value="20100430">(HEX)</td>
 </tr>
 <tr>
  <td rowspan=2><font size="+1">Device Unique</font></td>
  <td>Machine Name<br><textarea name="id_2" cols=50 rows=3>00-25-5C-BA-5A-00</textarea></td>
 </tr>
 <tr>
  <td>
   MAC Address<br>
   &nbsp;&nbsp;&nbsp;00-25-5C-BA-5A-00
  </td>
 </tr>
 <tr>
  <td colspan=2>
   <div align="center">
    <input type="hidden" name="op" value="0">
    <input type="submit" value="Write">&nbsp;&nbsp;<input type="reset" value="Form Reset">
   </div>
  </td>
 </tr>
</table>
</form>
<form method="POST" name="param_read" action="setup.cgi">
 <input type="hidden" name="op" value="1">
  <div align="center">
   <input type="submit" value="Read">
  </div>
</form>
</div>
<br>
<hr>
<a href="../">Homepage</a>
</body>
</HTML>"##;

	#[test]
	pub fn can_parse_setup_cgis() {
		let result = parse_response_body_from_setup(REAL_WORLD_SETUP_RESPONSE);
		assert!(
			result.is_ok(),
			"Failed to parse response body from `setup.cgi`:\n  {:?}",
			result,
		);
		let parameters = result.expect("impossible probably, past assert.");

		assert!(
			parameters.static_ip_address().is_none(),
			"Expected static ip address to be empty, is using DHCP",
		);
		assert_eq!(
			parameters.raw_static_ip_address(),
			"192.168.0.1"
				.parse::<Ipv4Addr>()
				.expect("Failed to parse test IP Address"),
		);
		assert_eq!(
			parameters.subnet_mask(),
			"255.255.255.0"
				.parse::<Ipv4Addr>()
				.expect("Failed to parse test IP Address"),
		);
		assert_eq!(
			parameters.default_gateway(),
			"0.0.0.0"
				.parse::<Ipv4Addr>()
				.expect("Failed to parse test IP Address"),
		);

		assert!(parameters.using_dhcp());
		assert!(!parameters.using_self_managed_dns());

		assert!(
			parameters.primary_dns().is_none(),
			"Expected Primary DNS to be empty, is not using self-managed DNS",
		);
		assert!(
			parameters.secondary_dns().is_none(),
			"Expected Primary DNS to be empty, is not using self-managed DNS",
		);
		assert_eq!(
			parameters.raw_primary_dns(),
			"0.0.0.0"
				.parse::<Ipv4Addr>()
				.expect("Failed to parse static IP Address for testing")
		);
		assert_eq!(
			parameters.raw_secondary_dns(),
			"0.0.0.0"
				.parse::<Ipv4Addr>()
				.expect("Failed to parse static IP Address for testing")
		);

		assert!(parameters.jumbo_frame());

		assert_eq!(
			parameters.host_pc_ip_address(),
			"0.0.0.0"
				.parse::<Ipv4Addr>()
				.expect("Failed to parse static IP Address for testing")
		);
		assert_eq!(parameters.hdd_bank_size(), CatDevBankSize::TwentyFiveGbs);
		assert_eq!(parameters.hdd_bank_no(), 0);

		assert_eq!(parameters.atapi_emulator_port(), 7974);
		assert_eq!(parameters.sdio_printf_port(), 7975);
		assert_eq!(parameters.sdio_block_port(), 7976);
		assert_eq!(parameters.exi_port(), 7977);
		assert_eq!(parameters.parameter_space_port(), 7978);

		assert!(!parameters.drive_timing_emulation_enabled());

		assert!(parameters.is_cat_dev_mode());
		assert!(!parameters.is_h_reader_mode());

		assert_eq!(parameters.drive_product_revision(), "0000");
		assert_eq!(parameters.drive_vendor_code(), "02");
		assert_eq!(parameters.drive_device_code(), "06");
		assert_eq!(parameters.drive_release_date(), "20100430");

		assert_eq!(parameters.device_name(), "00-25-5C-BA-5A-00");
		assert_eq!(
			parameters.mac_address(),
			"00-25-5C-BA-5A-00"
				.parse::<MacAddress>()
				.expect("Failed to parse static MacAddress for tests"),
		);
	}
}
