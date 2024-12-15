//! API Errors for MION CGI pages.

use miette::Diagnostic;
use thiserror::Error;

/// API Errors for interacting with MION CGI pages.
#[derive(Diagnostic, Error, Debug, PartialEq, Eq)]
pub enum MIONCGIApiError {
	/// There are only so many sizes a cat-dev HDD bank can come in, these are
	/// hardcoded. You've specified a bank we can't set.
	#[error("Unknown ID for Cat-DEV Bank Sizes: [{0}]")]
	#[diagnostic(code(cat_dev::api::mion::cgi::unknown_bank_size))]
	UnknownCatDevBankSizeId(u32),
	/// There are a series of operations you can call on `control.cgi`,
	/// unfortunately the one specified is not an operation we know on
	/// any firmware version.
	#[error("Unknown operation for `control.cgi`: [{0}]")]
	#[diagnostic(code(cat_dev::api::mion::cgi::control::unknown_operation))]
	UnknownControlOperation(String),
	/// There are a series of operations you can call on `status.cgi`,
	/// unfortunately the one specified is not an operation we know on
	/// any firmware version.
	#[error("Unknown operation for `status.cgi`: [{0}]")]
	#[diagnostic(code(cat_dev::api::mion::cgi::status::unknown_operation))]
	UnknownStatusOperation(String),
}
