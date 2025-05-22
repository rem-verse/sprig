//! Errors related to various PCFS protocols.

use miette::Diagnostic;
use thiserror::Error;

/// Error's specific to calling a specific PCFS API.
#[derive(Diagnostic, Error, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PCFSApiError {
	#[error("Mode string is expected to match '(r|w|a)b?+?', but did not! invalid mode: {0}")]
	#[diagnostic(code(cat_dev::api::fsmeul::pcfs::bad_mode_string))]
	BadModeString(String),
	/// `PCFS` Sata protocol only supports [`u32::MAX`] packet sizes because they
	/// store length of body as a [`u32`].
	#[error(
		"This packet body is too large to ever fit in a PCFS Sata Packet, is: 0x{0:02X?} bytes long, max is 0xFFFFFFFF!"
	)]
	#[diagnostic(code(cat_dev::api::fsemul::pcfs::packet_too_large_for_sata))]
	PacketTooLargeForSata(usize),
	#[error("The requested path: [{0}] is too long, paths must be no more than 511 bytes.")]
	#[diagnostic(code(cat_dev::api::fsemul::pcfs::path_too_long))]
	PathTooLong(String),
	/// Requested path is not inside of a mapped directory.
	#[error("The requested path: [{0}] was not inside of a mapped directory, cannot serve.")]
	#[diagnostic(code(cat_dev::api::fsemul::pcfs::path_not_mapped))]
	PathNotMapped(String),
	#[cfg(feature = "servers")]
	#[error(
		"The server was not configured correctly (programmer error), please report that extension: {0} did not load properly!"
	)]
	#[diagnostic(code(cat_dev::api::fsemul::pcfs::missing_server_extension))]
	MissingCriticalExtension(String),
}

/// Error serializing/deserializing the PCFS Sata protocol.
#[derive(Diagnostic, Error, Debug, PartialEq, Eq)]
pub enum SataProtocolError {
	#[error("Mode string is expected to match '(r|w|a)b?+?', but did not! invalid mode: {0}")]
	#[diagnostic(code(cat_dev::net::parse::pcfs::sata::bad_mode_string))]
	BadModeString(String),
	/// PCFS sata headers should always end with 8 NUL bytes for padding.
	#[error(
		"Packet header for PCFS should end with 8 bytes of padding (all 0x0), but was: [{0:02X?}]!"
	)]
	#[diagnostic(code(cat_dev::net::parse::pcfs::sata::header_bad_padding))]
	HeaderBadPadding([u8; 8]),
	/// SATA packets have fields that only the host pc is supposed to set.
	///
	/// One of these fields was set by the client instead of the host pc.
	#[error(
		"Packet header has fields that only the host should ever populate, but they were populated by the non-host: (timestamp: {0:02X}) / (pid: {1:02X}). This is a malformed packet from this device."
	)]
	#[diagnostic(code(cat_dev::net::parse::pcfs::sata::header_non_host_set_host_fields))]
	NonHostSetHostOnlyHeaderFields(u32, u32),
	/// Unknown type of packet received on PCFS Sata.
	#[error("Unknown type of packet for PCFS Sata: {0:02X}!")]
	#[diagnostic(code(cat_dev::net::parse::pcfs::sata::unknown_type))]
	UnknownPacketType(u32),
	/// Unknown type of query for getting a files information.
	#[error("Unknown query type for GetInfo operation: {0}")]
	#[diagnostic(code(cat_dev::net::parse::pcfs::sata::unknown_get_info_query_type))]
	UnknownGetInfoQueryType(u32),
	#[error("Unknown file location to move too: {0}")]
	#[diagnostic(code(cat_dev::net::parse::pcfs::sata::unknown_file_location))]
	UnknownFileLocation(u32),
}
