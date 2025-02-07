//! Error types specifically relevant to the ATAPI Protocol.

use bytes::Bytes;
use miette::Diagnostic;
use thiserror::Error;

/// Error serializing/deserializing the ATAPI protocol.
#[derive(Diagnostic, Error, Debug, PartialEq, Eq)]
pub enum ATAPIProtocolError {
	/// All ATAPI Client requests must be exactly 12 bytes long, this one was longer or shorter.
	#[error("All ATAPI Client requests are commands in exactly 12 bytes, this one was not, but was: {0:02X?}")]
	#[diagnostic(code(cat_dev::net::parse::fsemul::atapi::invalid_client_request_length))]
	InvalidClientRequestLength(Bytes),
}
