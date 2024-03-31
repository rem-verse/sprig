//! A container for all the types of errors generated crate-wide.
//!
//! The top level error type is: [`CatBridgeError`], which wraps all the other
//! types of errors. You can find more specific error types documented on each
//! specific item.

use bytes::Bytes;
use local_ip_address::Error as LocalIpAddressError;
use mac_address::MacParseError;
use miette::Diagnostic;
use reqwest::Error as ReqwestError;
use serde_urlencoded::ser::Error as SerdeUrlEncodeError;
use std::{net::AddrParseError, num::ParseIntError, string::FromUtf8Error, time::Duration};
use thiserror::Error;
use tokio::{io::Error as IoError, sync::mpsc::error::SendError, task::JoinError};

/// The 'top-level' error type for this entire crate, all error types
/// wrap underneath this.
#[derive(Error, Diagnostic, Debug)]
pub enum CatBridgeError {
	/// See [`APIError`] for details.
	#[error(transparent)]
	#[diagnostic(transparent)]
	ApiError(#[from] APIError),
	/// See [`FSError`] for details.
	#[error(transparent)]
	#[diagnostic(transparent)]
	FilesystemError(#[from] FSError),
	/// We spawned a background task, and for whatever reason we could not
	/// wait for it to finish.
	///
	/// For the potential reasons for this, take a peek at [`tokio`]'s
	/// documentation. Which is our asynchronous runtime.
	#[error("We could not await an asynchronous task we spawned: {0:?}")]
	#[diagnostic(code(cat_dev::join_failure))]
	JoinFailure(JoinError),
	/// See [`NetworkError`] for details.
	#[error(transparent)]
	#[diagnostic(transparent)]
	NetworkError(#[from] NetworkError),
	/// We tried sending a message from one thread to another (within the same
	/// process), but delivery could not be completed.
	///
	/// For more information on why this could fail please look at the associated
	/// modules we may be using:
	///
	/// - [`std::sync::mpsc`]
	/// - [`tokio::sync::mpsc`]
	///
	/// Each of these contain more information.
	#[error("We could not send a message locally to another part of the process. This channel must've been closed unexpectedly.")]
	#[diagnostic(code(cat_dev::closed_channel))]
	ClosedChannel,
	/// We tried to spawn a task to run in the background, but couldn't.
	///
	/// For the potential reasons for this, take a peek at [`tokio`]'s
	/// documentation. Which is our asynchronous runtime.
	#[error("We could not spawn a task (a lightweight thread) to do work on.")]
	#[diagnostic(code(cat_dev::spawn_failure))]
	SpawnFailure,
	#[error("This cat-dev API requires a 32 bit usize, and this machine does not have it, please upgrade.")]
	#[diagnostic(code(cat_dev::unsupported_bits_per_core))]
	UnsupportedBitsPerCore,
}

/// An error that comes from one of our APIs, e.g. passing in a parameter
/// that wasn't expected.
///
/// All the APIs within this crate will have errors will be collapsed under
/// this particular error type. There will be no inner separation between
/// modules.
#[derive(Error, Diagnostic, Debug, PartialEq, Eq)]
pub enum APIError {
	/// You attempted to encrypt data that we could not encrypt.
	#[error("We could not encrypt your data, because it was not padded to the correct length, expected a block size of: {0}")]
	#[diagnostic(code(cat_dev::api::bad_decrypted_data_length))]
	BadDecryptedDataLength(usize),
	/// You attempted to decrypt data that we could not decrypt.
	#[error("We could not decrypt your data, because it was not padded to the correct length, expected a block size of: {0}")]
	#[diagnostic(code(cat_dev::api::bad_encrypted_data_length))]
	BadEncryptedDataLength(usize),
	/// The MION Firmware files end with a final byte that acts as a checksum
	/// to validate the content before it was correct. Your checksum was not
	/// correct.
	#[error("The MION Firmware file you provided had an invalid checksum, we expected: {1:02x}, but got: {0:02x}")]
	#[diagnostic(code(cat_dev::api::mion_fw::bad_checksum))]
	BadMionFWChecksum(u8, u8),
	/// You attempted to set the default host bridge to a bridge that does not exist.
	#[error("You cannot set a default bridge that does not exist.")]
	#[diagnostic(code(cat_dev::api::default_device_must_exist))]
	DefaultDeviceMustExist,
	/// You attempted to create a MION device name, which did not contain ASCII
	/// characters.
	///
	/// MION Identities must contain all ascii characters.
	#[error("A device name has to be completely ASCII! But it wasn't ASCII!")]
	#[diagnostic(code(cat_dev::api::name_not_ascii))]
	DeviceNameMustBeAscii,
	/// You attempted to create a MION device name, but it was empty.
	///
	/// MION Identities must have at LEAST 1 byte.
	#[error("The device name cannot be empty, it must be at least one byte long.")]
	#[diagnostic(code(cat_dev::api::name_cannot_be_empty))]
	DeviceNameCannotBeEmpty,
	/// You attempted to create a MION device name, but it was longer than 255
	/// bytes.
	///
	/// A MION device name has to be serialized into a packet, where it's length
	/// is represented as a [`u8`] which means it can only be [`u8::MAX`], aka
	/// 255 bytes.
	#[error("A Device Name can only be 255 bytes long, but you specified one: {0} bytes long.")]
	#[diagnostic(code(cat_dev::api::name_too_long))]
	DeviceNameTooLong(usize),
	/// All MION Firmware files must be at a minimum 0x26 bytes long.
	///
	/// This covers a single AES-256 block (32 bytes), plus the 6 byte footer
	/// that they contain.
	#[error("The MION Firmware file provided was too small, it must be at least 0x26 bytes long, was {0:02x}")]
	#[diagnostic(code(cat_dev::api::mion_fw::too_small))]
	MionFirmwareTooSmall(usize),
	/// The MION Firmware version string must end with a `0x00`, as this is a
	/// load-bearing NUL terminator for many parts of the firmware.
	#[error("The Version String for MION Firmware Files Typed 'MION', must have their version bytes end with a NUL terminator (0x00) due to an oversight in programming. Your file ended with: ({0:02x})")]
	#[diagnostic(code(cat_dev::api::mion_fw::missing_nul_terminator))]
	MionFirmwareMissingNULTerminator(u8),
	/// All MION FW files must end with:
	///
	/// - `PWI-SS_FW_IMAGE` for IPL/MION firmware types
	/// - `PWI-SS_FP_IMAGE` for FPGA firmware types.
	///
	/// If they do not, they are immediately considered invalid.
	#[error("While validating the decrypted contents of your FW we were not able to identify the required ending bytes, this firmware is corrupt.")]
	#[diagnostic(code(cat_dev::api::mion_fw::missing_signature))]
	MionFirmwareMissingSignature,
	/// You tried asking for a parameter of a specific name, but we could not
	/// find a parameter with the name you specified.
	///
	/// We have created the concept of "name"'s for some parameters in the
	/// parameter space. Although the official CLI tools just used indexes, I
	/// in particular find indexes hard to remember so wanted to ensure folks
	/// could just "say" what they wanted to lookup. Of course though not every
	/// field is named, nor does it mean the API was given a non typo'd value.
	#[error("The MION Parameter name: {0} is not known, cannot find index.")]
	#[diagnostic(code(cat_dev::api::parameter::name_not_known))]
	MIONParameterNameNotKnown(String),
	/// You tried asking for a parameter that does not exist.
	///
	/// There are only 512 parameters, so you can only ask for parameters in
	/// (0-511) inclusive.
	#[error("You asked for the MION Parameter at index: {0}, but MION Parameter indexes cannot be greater than 511.")]
	#[diagnostic(code(cat_dev::api::parameter::not_in_range))]
	MIONParameterNotInRage(usize),
	/// You passed a parameter space to an API that requires the full parameter
	/// space, but it was not the correct length (512 bytes).
	#[error("The MION Parameter body you passed in was: {0} bytes long, but must be exactly 512 bytes long!")]
	#[diagnostic(code(cat_dev::api::parameter::body_incorrect_length))]
	MIONParameterBodyNotCorrectLength(usize),
	/// We failed to find our own hosts local IP address.
	///
	/// This usually means we don't have a network interface we can communicate
	/// on that has an IPv4 address assigned.
	#[error("We could not find the local hosts ipv4 address which is needed if an ip isn't explicitly passed in.")]
	#[diagnostic(code(cat_dev::api::no_host_ip_found))]
	NoHostIpFound,
	/// There are a series of operations you can call on `control.cgi`,
	/// unfortunately the one specified is not an operation we know on
	/// any firmware version.
	#[error("Unknown operation for `control.cgi`: [{0}]")]
	#[diagnostic(code(cat_dev::api::control::unknown_operation))]
	UnknownControlOperation(String),
	#[error("Unknown ID for Cat-DEV Bank Sizes: [{0}]")]
	#[diagnostic(code(cat_dev::api::setup::unknown_bank_size))]
	UnknownCatDevBankSizeId(u32),
}

/// Trying to interact with the filesystem has resulted in an error.
#[derive(Error, Diagnostic, Debug)]
pub enum FSError {
	/// We need a place to read/store a list of all the bridges on your host.
	///
	/// However, if you see this we weren't able to automatically determine where
	/// that file should go. Please either contribute a path for your OS to use,
	/// or manually provide the host bridge path (this can only be done on the
	/// newer versions of tools).
	#[error("We can't find the path to store a complete list of host-bridges, please use explicit paths instead.")]
	#[diagnostic(code(cat_dev::fs::cant_find_hostenv_path))]
	CantFindHostEnvPath,
	#[error(
		"We can't find the path to store fsemul configuration, please use explicit paths instead."
	)]
	#[diagnostic(code(cat_dev::fs::cant_find_fsemul_path))]
	CantFindFsEmulPath,
	#[error("We can't find the root Cafe SDK directory, please use explicit paths instead.")]
	#[diagnostic(code(cat_dev::fs::cant_find_cafe_sdk_path))]
	CantFindCafeSdkPath,
	#[error("The Cafe SDK Path does not have the appropriate MLC path directories.")]
	#[diagnostic(code(cat_dev::fs::corrupt_cafe_sdk_path))]
	CafeSdkPathCorrupt,
	/// We expected to read UTF-8 data from the filesystem, but it wasn't UTF-8.
	#[error("Data read from the filesystem was expected to be UTF-8, but was not: {0}")]
	#[diagnostic(code(cat_dev::fs::utf8_expected))]
	InvalidDataNeedsUTF8(#[from] FromUtf8Error),
	/// We expected to parse file as an INI data, but it did not contain valid
	/// INI data.
	#[error("Data read from the filesystem was expected to be a valid INI file: {0}")]
	#[diagnostic(code(cat_dev::fs::expected_ini))]
	InvalidDataNeedsToBeINI(String),
	/// See [`tokio::io::Error`] for details.
	#[error("Error writing/reading data from the filesystem: {0}")]
	#[diagnostic(code(cat_dev::fs::io_failure))]
	IOError(#[from] IoError),
	/// File "magic" are generally constants that should always be true.
	#[error("Expected file magic of: {0}, got {1} as magic bytes")]
	#[diagnostic(code(cat_dev::fs::file_magic))]
	InvalidFileMagic(u32, u32),
	/// Expected a file sized a specific amount of bytes, and it wasn't.
	#[error("Expected file size of: {0} bytes, got a file sized {1} bytes")]
	#[diagnostic(code(cat_dev::fs::invalid_file_size))]
	InvalidFileSize(usize, usize),
	/// The file needs to be a certain amount of bytes, and it wasn't.
	#[error("File needs to be at least: {0} bytes, is {1} bytes")]
	#[diagnostic(code(cat_dev::fs::too_small))]
	TooSmall(usize, usize),
	/// The file can't be larger than a certain amount of bytes, and it was.
	#[error("File cannot be larger than: {0} bytes, is {1} bytes")]
	#[diagnostic(code(cat_dev::fs::too_large))]
	TooLarge(usize, usize),
}

/// Trying to interact with the network has resulted in an error.
///
/// *NOTE: this does not cover bogus data coming in from the network. This only
/// covers errors related to interacting with the network. If you're looking
/// for bogus data from the network errors look at [`NetworkParseError`].*
#[derive(Error, Diagnostic, Debug)]
pub enum NetworkError {
	/// We failed to bind to a local address to listen for packets from the
	/// network.
	///
	/// This can happen for numerous reason, such as:
	///
	/// - The program does not have permission to listen on this specific port.
	/// - The address is already being used by another process.
	/// - The network interface returned some type of error.
	///
	/// There are multiple other cases, but in general they're pretty OS
	/// specific.
	#[error("Failed to bind to a local address to receive packets.")]
	#[diagnostic(code(cat_dev::net::bind_address_error))]
	BindAddressError,
	/// See [`NetworkParseError`] for details.
	#[error(transparent)]
	#[diagnostic(transparent)]
	ParseError(#[from] NetworkParseError),
	/// See [`tokio::io::Error`] for details.
	#[error("Error talking to the network could not send/receive data: {0}")]
	#[diagnostic(code(cat_dev::net::native_failure))]
	IOError(#[from] IoError),
	/// See [`network_interface::Error::GetIfAddrsError`] for details.
	#[error("Failed to list the network interfaces on your device.")]
	#[diagnostic(code(cat_dev::net::list_interfaces_error))]
	ListInterfacesError,
	/// If we failed to call `setsockopt` through libc.
	///
	/// For example if on linux see: <https://linux.die.net/man/2/setsockopt>
	#[error("Failed to set the socket we're bound on as a broadcast address, this is needed to discover CAT devices.")]
	#[diagnostic(code(cat_dev::net::set_broadcast_failure))]
	SetBroadcastFailure,
	/// We waited too long to send/receive data from the network.
	///
	/// There may be something wrong with our network connection, or the targets
	/// network connection.
	#[error(
		"Timed out while writing/reading data from the network, failed to send and receive data."
	)]
	#[diagnostic(code(cat_dev::net::timeout))]
	TimeoutError,
	/// See [`reqwest::Error`] for details.
	#[error("Underlying HTTP client error: {0}")]
	#[diagnostic(code(cat_dev::net::http_failure))]
	ReqwestError(#[from] ReqwestError),
	/// See [`local_ip_address::Error`] for details.
	#[error("Failure fetching local ip address: {0}")]
	#[diagnostic(code(cat_dev::net::local_ip_failure))]
	LocalIpError(#[from] LocalIpAddressError),
	#[error("Error creating a new connection, timed out after: {0:?}, perhaps the device isn't available?")]
	#[diagnostic(code(cat_dev::net::connection_timeout))]
	ConnectionTimeout(Duration),
	#[error("Error looking up PCFS Address: {0} cannot read/write.")]
	#[diagnostic(code(cat_dev::net::unknown_pcfs_server_address))]
	UnknownPCFSServerAddress(u32),
	#[error("Error queueing up packet to be sent out over a conenction: {0:?}")]
	#[diagnostic(code(cat_dev::net::send_queue_failure))]
	SendQueueFailure(#[from] SendError<Bytes>),
}

/// We tried parsing some data from the network, but failed to do so, someone
/// sent us some junk.
#[derive(Error, Diagnostic, Debug, PartialEq, Eq)]
pub enum NetworkParseError {
	/// A field encoded within a packet was not correct (e.g. a string wasn't
	/// UTF-8).
	#[error("Reading Field {1} from Packet {0}, was not encoded correctly must be encoded as {2}")]
	#[diagnostic(code(cat_dev::net::parse::field_encoded_incorrectly))]
	FieldEncodedIncorrectly(&'static str, &'static str, &'static str),
	/// A field encoded within a packet requires a minimum number of bytes, but
	/// the field was not long enough.
	#[error("Tried Reading Field {1} from Packet {0}. This Field requires at least {2} bytes, but only had {3}, bytes: {4:02x?}")]
	#[diagnostic(code(cat_dev::net::parse::field_not_long_enough))]
	FieldNotLongEnough(&'static str, &'static str, usize, usize, Bytes),
	/// A field encoded within a packet has a maximum length that was exceeded.
	#[error("Tried Reading Field {1} from Packet {0}. This field is at max {2} bytes, but had {3}, bytes: {4:02x?}")]
	#[diagnostic(code(cat_dev::net::parse::field_too_long))]
	FieldTooLong(&'static str, &'static str, usize, usize, Bytes),
	/// The overall size of the packet was too short, and we cannot successfully
	/// parse it.
	#[error("Tried to read Packet of type ({0}) from network needs at least {1} bytes, but only got {2} bytes: {3:02x?}")]
	#[diagnostic(code(cat_dev::net::parse::not_enough_data))]
	NotEnoughData(&'static str, usize, usize, Bytes),
	/// We expected to read a packet containing exactly a set of bytes,
	/// unfortunatley it did not contain those _Exact_ bytes.
	#[error("Tried to read Packet of type ({0}) from network, must be encoded exactly as [{1:02x?}], but got [{2:02x?}]")]
	#[diagnostic(code(cat_dev::net::parse::packet_doesnt_match_static_data))]
	PacketDoesntMatchStaticPayload(&'static str, &'static [u8], Bytes),
	/// The overall size of the packet was too long, and there was unexpected
	/// data at the end, a.k.a. the "Trailer".
	#[error("Unexpected Trailer for Packet `{0}` received from the network (we're not sure what do with this extra data), extra bytes: {1:02x?}")]
	#[diagnostic(code(cat_dev::net::parse::unexpected_trailer))]
	UnexpectedTrailer(&'static str, Bytes),
	/// Unknown Packet Type/Command.
	#[error("Unknown Code aka Packet Type: `{0}` received from the network (this may mean your CAT-DEV is doing something we didn't expect)")]
	#[diagnostic(code(cat_dev::net::parse::unknown_packet_type))]
	UnknownCommand(u8),
	/// Unknown packet type for the MION Params port.
	#[error("Unknown Packet Type: `{0}` received from the network (this may mean your CAT-DEV is doing something we didn't expect)")]
	#[diagnostic(code(cat_dev::net::parse::params::unknown_packet_type))]
	UnknownParamsPacketType(i32),
	/// We got an error code back from trying to interact with the MION
	/// paramspace port.
	///
	/// We unfortunately do not have these error codes known at this point in
	/// time.
	#[error("Error code received from MION Params: `{0}`")]
	#[diagnostic(code(cat_dev::net::parse::params::error_code))]
	ParamsPacketErrorCode(i32),
	/// See [`serde_urlencoded::ser::Error`] for details.
	#[error("Failed to encode data as form data: {0}")]
	#[diagnostic(code(cat_dev::net::parse::http::encode::form_data_error))]
	FormDataEncodeError(#[from] SerdeUrlEncodeError),
	/// We got an unexpected status code from the CAT-DEV, this is only used when
	/// we did not get an HTTP body back from the CAT-DEV as well.
	#[error("Got an unexpected status code that wasn't successful over HTTP: {0}")]
	#[diagnostic(code(cat_dev::net::parse::http::bad_status_code_without_body))]
	UnexpectedStatusCodeNoBody(u16),
	/// We got an unexpected status code from the CAT-DEV, it also came with an
	/// HTTP body that may contain clues to it's error.
	#[error("Got an unexpected status code that wasn't successful over HTTP: {0}, Body: {1:02x?}")]
	UnexpectedStatusCode(u16, Bytes),
	/// We expected to read UTF-8 data from the network, but it wasn't UTF-8.
	#[error("Data read from the network was expected to be UTF-8, but was not: {0}")]
	#[diagnostic(code(cat_dev::net::parse::utf8_expected))]
	InvalidDataNeedsUTF8(#[from] FromUtf8Error),
	/// We could not find the `<body>` tags in a page that is supposed to return
	/// HTML.
	#[error("Could not parse HTML response could not find one of the body tags: `<body>`, or `</body>`: {0}")]
	#[diagnostic(code(cat_dev::net::parse::html::no_body_tag))]
	HtmlResponseMissingBody(String),
	#[error("Could not find Memory Dump Table Body, failed to find sigils: {0}")]
	#[diagnostic(code(cat_dev::net::parse::html::no_mem_dump_sigil))]
	HtmlResponseMissingMemoryDumpSigil(String),
	/// The HTML response we got was expected to contain hexadecimal bytes.
	///
	/// We could not parse one of these hexadecimal bytes. Either the device
	/// responded with an error we didn't properly pick up on, or we got corrupt
	/// data somehow.,
	#[error("Could not parse byte from memory dump: {0}")]
	#[diagnostic(code(cat_dev::net::parse::html::bad_memory_byte))]
	HtmlResponseBadByte(String),
	#[error("Could not find input with name: `{0}`, within HTML body: `{1}`")]
	#[diagnostic(code(cat_dev::net::parse::html::missing_tagged_input))]
	HtmlResponseMissingTaggedInput(String, String),
	#[error("Expected HTML Response to have an IP as a string, but could not parse: `{0:?}`")]
	#[diagnostic(code(cat_dev::net::parse::html::html_response_ip_encoding_error))]
	HtmlResponseIpExpectedButNotThere(AddrParseError),
	#[error("Expected HTML Response to have a radio button, could not parse: `{0}`")]
	HtmlResponseNoRadioChecked(String),
	#[error("Expected HTML Response to have an number as a string, but could not parse: `{0:?}`")]
	#[diagnostic(code(cat_dev::net::parse::html::html_response_number_encoding_error))]
	HtmlResponseNumberExpectedButNotThere(ParseIntError),
	#[error("Expected HTML Response to have a table item with prefix: {1}, but couldn't find one in: `{0}`")]
	HtmlResponseNoTableItemWithPrefix(String, String),
	#[error("Expected HTML Response to have a MAC as a string, but could not parse: `{0:?}`")]
	#[diagnostic(code(cat_dev::net::parse::html::html_response_mac_encoding_error))]
	HtmlResponseMacExpectedButNotThere(MacParseError),
	#[error("Got an unexpected sdio/printf packet type: {0}, not sure how to handle")]
	#[diagnostic(code(cat_dev::net::parse::sdio::printf::unknown_packet_type))]
	UnknownSdioPrintfPacketType(u8),
	#[error("Got an unexpected message fragment from an sdio printf packet type: {0}, not sure how to handle.")]
	#[diagnostic(code(cat_dev::net::parse::sdio::printf::unknown_message_type))]
	UnknownSdioPrintfMessageType(u16),
	#[error("Got an invalid channel to read/write from for sdio/printf, (first byte: {0:02x} should be <0xC), (full channel: {1})")]
	#[diagnostic(code(cat_dev::net::parse::sdio::printf::invalid_channel))]
	SdioPrintfInvalidChannel(u8, u32),
	#[error("Packet headed for SDIO PRINTF/CONTROL was size {0}, but needs to be 512 bytes")]
	#[diagnostic(code(cat_dev::net::parse::sdio::printf::invalid_sized_packet))]
	SdioPrintfInvalidSize(usize),
}
