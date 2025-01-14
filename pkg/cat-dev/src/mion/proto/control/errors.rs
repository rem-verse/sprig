//! Error types related to MION Control related data.

use miette::Diagnostic;
use thiserror::Error;

/// Errors specifically for MION Control Protocol related errors.
#[derive(Diagnostic, Error, Debug, PartialEq, Eq)]
pub enum MIONControlProtocolError {
	/// Unknown Packet Type/Command.
	#[error("Unknown Code aka Packet Type for MION CONTROL: `{0}` received from the network (this may mean your CAT-DEV is doing something we didn't expect)")]
	#[diagnostic(code(cat_dev::net::parse::mion::control::unknown_packet_type))]
	UnknownCommand(u8),
}
