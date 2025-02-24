//! Errors related to SDIO protocols.

use miette::Diagnostic;
use thiserror::Error;

/// An API Error for interacting with the SDIO client.
#[derive(Diagnostic, Error, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SDIOAPIError {
	/// Invalid LBA has been supplied, you might want to set the raw LBA.
	#[error("The LBA provided was invalid, must be divisble by 512 bytes: {0}")]
	#[diagnostic(code(cat_dev::api::fsemul::sdio::invalid_lba))]
	InvalidLBA(u32),
	/// An invalid channel name has been supplied.
	#[error("The channel provided was invalid, (first byte: {0:02x}, should be <0xC), (full: {1})")]
	#[diagnostic(code(cat_dev::api::fsemul::sdio::invalid_channel))]
	InvalidChannel(u8, u32),
	#[cfg(feature = "clients")]
	#[error("Cannot call `serve` on `SdioServer` twice!")]
	#[diagnostic(code(cat_dev::api::fsemul::sdio::cannot_serve_server_twice))]
	CannotServeServerTwice,
}

/// Error serializing/deserializing the SDIO protocol.
#[derive(Diagnostic, Error, Debug, PartialEq, Eq)]
pub enum SDIOProtocolError {
	/// SDIO addresses are out of range.
	#[error("The SDIO client requested address: {0:02X}, but max address was: {1:02X}")]
	#[diagnostic(code(cat_dev::net::parse::fsemul::sdio::address_out_of_range))]
	AddressOutOfRange(u128, u128),
	/// The first part of a 'printf' message is a channel identifier which is
	/// like a stream identifier (think differences between STDIN/STDOUT/STDERR).
	///
	/// Unfortunately the channel specified was invalid.
	#[error(
		"Got an invalid channel to read/write from for sdio/printf, (first byte: {0:02x} should be <0xC), (full channel: {1})"
	)]
	#[diagnostic(code(cat_dev::net::parse::fsemul::sdio::printf::invalid_channel))]
	PrintfInvalidChannel(u8, u32),
	/// All SDIO Printf messages must be 512 bytes long, this one was not.
	#[error("Packet headed for SDIO PRINTF/CONTROL was size {0}, but needs to be 512 bytes")]
	#[diagnostic(code(cat_dev::net::parse::fsemul::sdio::printf::invalid_sized_packet))]
	PrintfInvalidSize(usize),
	/// Unknown command/packet type came over the SDIO protocol.
	#[error("Got an unexpected sdio/printf packet type: {0}, not sure how to handle.")]
	#[diagnostic(code(cat_dev::net::parse::fsemul::sdio::printf::unknown_packet_type))]
	UnknownPrintfPacketType(u8),
	/// A 'printf' message, has an inner message type, which we didn't recognize.
	#[error(
		"Got an unexpected message fragment from an sdio printf packet type: {0}, not sure how to handle."
	)]
	#[diagnostic(code(cat_dev::net::parse::fsemul::sdio::printf::unknown_message_type))]
	UnknownPrintfMessageType(u16),
}
