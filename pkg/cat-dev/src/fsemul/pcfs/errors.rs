//! Errors related to various PCFS protocols.

use miette::Diagnostic;
use thiserror::Error;

/// Error's specific to calling a specific PCFS API.
#[derive(Diagnostic, Error, Debug, PartialEq, Eq)]
pub enum PCFSApiError {
	/// The encoded packet had a very invalid packet size.
	#[error("A packet attempting to be encoded as a PCFS packet said it contained {0:02X} bytes, but it actually contained {1:02X} bytes.")]
	#[diagnostic(code(cat_dev::api::fsemul::pcfs::incorrect_size_packet))]
	IncorrectSizePacket(usize, usize),
	/// Cannot encode a packet without a header.
	#[error("A packet attempting to be encoded as a PCFS packet must have at least 0x20 bytes, but only had: 0x{0:02X} bytes.")]
	#[diagnostic(code(cat_dev::api::fsemul::pcfs::packet_missing_header))]
	PacketMissingHeader(usize),
}

/// Error serializing/deserializing the PCFS Sata protocol.
#[derive(Diagnostic, Error, Debug, PartialEq, Eq)]
pub enum PCFSSataProtocolError {
	/// PCFS sata headers should always end with 8 NUL bytes for padding.
	#[error(
		"Packet header for PCFS should end with 8 bytes of padding (all 0x0), but was: [{0:02X?}]!"
	)]
	#[diagnostic(code(cat_dev::net::parse::pcfs::sata::header_bad_padding))]
	HeaderBadPadding([u8; 8]),
}
