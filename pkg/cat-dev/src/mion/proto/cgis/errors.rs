//! Errors related to CGI, and HTML pages for the MION.

use bytes::Bytes;
use mac_address::MacParseError;
use miette::Diagnostic;
use serde_urlencoded::ser::Error as SerdeUrlEncodeError;
use std::{net::AddrParseError, num::ParseIntError};
use thiserror::Error;

/// Errors related to handling and dealing with the HTML CGI pages.
#[derive(Debug, Diagnostic, Error, PartialEq, Eq)]
pub enum MIONCGIErrors {
	/// See [`serde_urlencoded::ser::Error`] for details.
	#[error("Failed to encode data as form data: {0}")]
	#[diagnostic(code(cat_dev::net::parse::mion::cgi::encode::form_data_error))]
	FormDataEncodeError(#[from] SerdeUrlEncodeError),
	/// The HTML response we got was expected to contain hexadecimal bytes.
	///
	/// We could not parse one of these hexadecimal bytes. Either the device
	/// responded with an error we didn't properly pick up on, or we got corrupt
	/// data somehow.,
	#[error("Could not parse byte from memory dump: {0}")]
	#[diagnostic(code(cat_dev::net::parse::mion::cgi::bad_memory_byte))]
	HtmlResponseBadByte(String),
	/// We expected this HTML response to have an IP encoded as a string, but we
	/// could not parse the string as an IP.
	#[error("Expected HTML Response to have an IP as a string, but could not parse: `{0:?}`")]
	#[diagnostic(code(cat_dev::net::parse::mion::cgi::html_response_ip_encoding_error))]
	HtmlResponseIpExpectedButNotThere(AddrParseError),
	/// We expected this HTML response to have a MAC Address encoded as a string,
	/// but we could not parse the string as a MAC address.
	#[error("Expected HTML Response to have a MAC as a string, but could not parse: `{0:?}`")]
	#[diagnostic(code(cat_dev::net::parse::mion::cgi::html_response_mac_encoding_error))]
	HtmlResponseMacExpectedButNotThere(MacParseError),
	/// We could not find the `<body>` tags in a page that is supposed to return
	/// HTML.
	#[error(
		"Could not parse HTML response could not find one of the body tags: `<body>`, or `</body>`: {0}"
	)]
	#[diagnostic(code(cat_dev::net::parse::mion::cgi::no_body_tag))]
	HtmlResponseMissingBody(String),
	#[error("Expected to find closing tag: {0}, in the rest of the HTML Body: {1}")]
	#[diagnostic(code(cat_dev::net::parse::mion::cgi::html_response_missing_closing_tag))]
	HtmlResponseMissingClosingTag(String, String),
	#[error("Could not find Memory Dump Table Body, failed to find sigils: {0}")]
	#[diagnostic(code(cat_dev::net::parse::mion::cgi::no_mem_dump_sigil))]
	HtmlResponseMissingMemoryDumpSigil(String),
	/// We attempted to find an `<input>` element with a certain name in the
	/// HTML response, but were not able to find one.
	#[error("Could not find input with name: `{0}`, within HTML body: `{1}`")]
	#[diagnostic(code(cat_dev::net::parse::mion::cgi::missing_tagged_input))]
	HtmlResponseMissingTaggedInput(String, String),
	#[error(
		"Expected to find a string to help identify the version in the HTML ({0}) as part of the string ({1}), but did not find one."
	)]
	#[diagnostic(code(cat_dev::net::parse::mion::cgi::html_response_missing_version_prefix))]
	HtmlResponseMissingVersionPart(String, String),
	#[error(
		"When fetching the versions of the MION we expect to find both the FW version, and the FPGA version, but only found the following versions: {0:?}"
	)]
	#[diagnostic(code(cat_dev::net::parse::mion::cgi::html_response_missing_versions))]
	HtmlResponseMissingVersions(Vec<String>),
	/// We expected the HTML response to have one radio box checked out of all
	/// the radio boxes, but we found no radio boxes that were checked.
	#[error("Expected HTML Response to have a radio button, could not parse: `{0}`")]
	#[diagnostic(code(cat_dev::net::parse::mion::cgi::html_radio_wasnt_checked))]
	HtmlResponseNoRadioChecked(String),
	/// We expected to find an item in a table `<tr>`/`<td>`, but were not able
	/// to find one in the HTML response we got back.
	#[error(
		"Expected HTML Response to have a table item with prefix: {1}, but couldn't find one in: `{0}`"
	)]
	HtmlResponseNoTableItemWithPrefix(String, String),
	/// We expected this HTML response to have a number encoded as a string, but we
	/// could not parse the string as a number.
	#[error("Expected HTML Response to have an number as a string, but could not parse: `{0:?}`")]
	#[diagnostic(code(cat_dev::net::parse::mion::cgi::html_response_number_encoding_error))]
	HtmlResponseNumberExpectedButNotThere(ParseIntError),
	/// We got an unexpected status code from the CAT-DEV, it also came with an
	/// HTTP body that may contain clues to it's error.
	#[error("Got an unexpected status code that wasn't successful over HTTP: {0}, Body: {1:02x?}")]
	#[diagnostic(code(cat_dev::net::parse::mion::cgi::status_code))]
	UnexpectedStatusCode(u16, Bytes),
	/// We got an unexpected status code from the CAT-DEV, this is only used when
	/// we did not get an HTTP body back from the CAT-DEV as well.
	#[error("Got an unexpected status code that wasn't successful over HTTP: {0}")]
	#[diagnostic(code(cat_dev::net::parse::http::bad_status_code_without_body))]
	UnexpectedStatusCodeNoBody(u16),
}
