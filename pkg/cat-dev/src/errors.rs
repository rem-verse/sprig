//! A container for all the types of errors generated crate-wide.
//!
//! The top level error type is: [`CatBridgeError`], which wraps all the other
//! types of errors. You can find more specific error types documented on each
//! specific item.

use crate::{
	fsemul::errors::{FSEmulAPIError, FSEmulFSError, FSEmulProtocolError},
	mion::errors::{MIONAPIError, MIONProtocolError},
};
use bytes::Bytes;
use local_ip_address::Error as LocalIpAddressError;
use miette::Diagnostic;
use network_interface::Error as NetworkInterfaceError;
use reqwest::Error as ReqwestError;
use std::{ffi::FromBytesUntilNulError, str::Utf8Error, string::FromUtf8Error, time::Duration};
use thiserror::Error;
use tokio::{io::Error as IoError, sync::mpsc::error::SendError, task::JoinError};
use walkdir::Error as WalkdirError;

/// The 'top-level' error type for this entire crate, all error types
/// wrap underneath this.
#[derive(Error, Diagnostic, Debug)]
pub enum CatBridgeError {
	/// See [`APIError`] for details.
	#[error(transparent)]
	#[diagnostic(transparent)]
	API(#[from] APIError),
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
	/// See [`FSError`] for details.
	#[error(transparent)]
	#[diagnostic(transparent)]
	FS(#[from] FSError),
	/// We spawned a background task, and for whatever reason we could not
	/// wait for it to finish.
	///
	/// For the potential reasons for this, take a peek at [`tokio`]'s
	/// documentation. Which is our asynchronous runtime.
	#[error("We could not await an asynchronous task we spawned: {0:?}")]
	#[diagnostic(code(cat_dev::join_failure))]
	JoinFailure(#[from] JoinError),
	/// See [`NetworkError`] for details.
	#[error(transparent)]
	#[diagnostic(transparent)]
	Network(#[from] NetworkError),
	/// We tried to spawn a task to run in the background, but couldn't.
	///
	/// For the potential reasons for this, take a peek at [`tokio`]'s
	/// documentation. Which is our asynchronous runtime.
	#[error("We could not spawn a task (a lightweight thread) to do work on.")]
	#[diagnostic(code(cat_dev::spawn_failure))]
	SpawnFailure(IoError),
	#[error("This cat-dev API requires a 32 bit usize, and this machine does not have it, please upgrade your machine.")]
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
	#[error(transparent)]
	#[diagnostic(transparent)]
	FSEmul(#[from] FSEmulAPIError),
	#[error(transparent)]
	#[diagnostic(transparent)]
	MION(#[from] MIONAPIError),
	/// We failed to find our own hosts local IP address.
	///
	/// This usually means we don't have a network interface we can communicate
	/// on that has an IPv4 address assigned.
	#[error("We could not find the local hosts ipv4 address which is needed if an ip isn't explicitly passed in.")]
	#[diagnostic(code(cat_dev::api::no_host_ip_found))]
	NoHostIpFound,
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
	#[error(transparent)]
	#[diagnostic(transparent)]
	FSEmul(#[from] FSEmulFSError),
	/// We expected to parse file as an INI data, but it did not contain valid
	/// INI data.
	#[error("Data read from the filesystem was expected to be a valid INI file: {0}")]
	#[diagnostic(code(cat_dev::fs::expected_ini))]
	InvalidDataNeedsToBeINI(String),
	/// File "magic" are generally constants that should always be true.
	#[error("Expected file magic of: {0}, got {1} as magic bytes")]
	#[diagnostic(code(cat_dev::fs::invalid_file_magic))]
	InvalidFileMagic(u32, u32),
	/// Expected a file sized a specific amount of bytes, and it wasn't.
	#[error("Expected file size of: {0} bytes, got a file sized {1} bytes")]
	#[diagnostic(code(cat_dev::fs::invalid_file_size))]
	InvalidFileSize(usize, usize),
	/// See [`tokio::io::Error`] for details.
	#[error("Error writing/reading data from the filesystem: {0}")]
	#[diagnostic(code(cat_dev::fs::io))]
	IO(#[from] IoError),
	#[error("Error iterating through directory: {0:?}")]
	#[diagnostic(code(cat_dev::fs::iterating_directory_error))]
	IteratingDirectoryError(#[from] WalkdirError),
	#[error("Expect file to have at least: {0} line(s), but it was only: {1} line(s) long.")]
	#[diagnostic(code(cat_dev::fs::too_few_lines))]
	TooFewLines(usize, usize),
	/// The file can't be larger than a certain amount of bytes, and it was.
	#[error("File cannot be larger than: {0} bytes, is {1} bytes")]
	#[diagnostic(code(cat_dev::fs::too_large))]
	TooLarge(usize, usize),
	/// The file needs to be a certain amount of bytes, and it wasn't.
	#[error("File needs to be at least: {0} bytes, is {1} bytes")]
	#[diagnostic(code(cat_dev::fs::too_small))]
	TooSmall(usize, usize),
	/// We expected to read UTF-8 data from the filesystem, but it wasn't UTF-8.
	#[error("Data read from the filesystem was expected to be UTF-8, but was not: {0}")]
	#[diagnostic(code(cat_dev::fs::utf8_expected))]
	Utf8Expected(#[from] FromUtf8Error),
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
	#[diagnostic(code(cat_dev::net::bind_failure))]
	BindFailure,
	/// See [`reqwest::Error`] for details.
	#[error("Underlying HTTP client error: {0}")]
	#[diagnostic(code(cat_dev::net::http_failure))]
	HTTP(#[from] ReqwestError),
	/// See [`tokio::io::Error`] for details.
	#[error("Error talking to the network could not send/receive data: {0}")]
	#[diagnostic(code(cat_dev::net::io_error))]
	IO(#[from] IoError),
	/// See [`network_interface::Error::GetIfAddrsError`] for details.
	#[error("Failed to list the network interfaces on your device: {0:?}.")]
	#[diagnostic(code(cat_dev::net::list_interfaces_error))]
	ListInterfacesFailure(NetworkInterfaceError),
	/// See [`local_ip_address::Error`] for details.
	#[error("Failure fetching local ip address: {0}")]
	#[diagnostic(code(cat_dev::net::local_ip_failure))]
	LocalIp(#[from] LocalIpAddressError),
	/// See [`NetworkParseError`] for details.
	#[error(transparent)]
	#[diagnostic(transparent)]
	Parse(#[from] NetworkParseError),
	/// The client requested too many bytes to actively serve.
	#[error("The client requested too many bytes to send over a connection at once (larger than usize::MAX on your architecture): {0}")]
	#[diagnostic(code(cat_dev::net::requested_size_too_large))]
	RequestedSizeTooLarge(u128),
	/// If we failed to call `setsockopt` through libc.
	///
	/// For example if on linux see: <https://linux.die.net/man/2/setsockopt>
	#[error("Failed to set the socket we're bound on as a broadcast address, this is needed to discover CAT devices.")]
	#[diagnostic(code(cat_dev::net::set_broadcast_failure))]
	SetBroadcastFailure,
	/// Error adding a packet to a queue to send.
	#[error("Error queueing up packet to be sent out over a conenction: {0:?}")]
	#[diagnostic(code(cat_dev::net::send_queue_failure))]
	SendQueueFailure(#[from] SendError<Bytes>),
	/// We waited too long to send/receive data from the network.
	///
	/// There may be something wrong with our network connection, or the targets
	/// network connection.
	#[error(
		"Timed out while writing/reading data from the network, failed to send and receive data."
	)]
	#[diagnostic(code(cat_dev::net::timeout))]
	Timeout(Duration),
}

impl From<ReqwestError> for CatBridgeError {
	fn from(value: ReqwestError) -> Self {
		Self::Network(value.into())
	}
}

impl From<SendError<Bytes>> for CatBridgeError {
	fn from(value: SendError<Bytes>) -> Self {
		Self::Network(value.into())
	}
}

/// We tried parsing some data from the network, but failed to do so, someone
/// sent us some junk.
#[derive(Error, Diagnostic, Debug, PartialEq, Eq)]
pub enum NetworkParseError {
	/// Failed reading C String with NUL bytes at the end.
	#[error("Failed reading c style string from packet: {0:?}")]
	#[diagnostic(code(cat_dev::net::parse::bad_c_string))]
	BadCString(#[from] FromBytesUntilNulError),
	/// We expected to read a packet containing exactly a set of bytes,
	/// unfortunatley it did not contain those _Exact_ bytes.
	#[error("Tried to read Packet of type ({0}) from network, must be encoded exactly as [{1:02x?}], but got [{2:02x?}]")]
	#[diagnostic(code(cat_dev::net::parse::doesnt_match_static_data))]
	DoesntMatchStaticPayload(&'static str, &'static [u8], Bytes),
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
	#[error(transparent)]
	#[diagnostic(transparent)]
	FSEmul(#[from] FSEmulProtocolError),
	/// Errors related to parsing MION specific protocols.
	#[error(transparent)]
	#[diagnostic(transparent)]
	MION(#[from] MIONProtocolError),
	/// The overall size of the packet was too short, and we cannot successfully
	/// parse it.
	#[error("Tried to read Packet of type ({0}) from network needs at least {1} bytes, but only got {2} bytes: {3:02x?}")]
	#[diagnostic(code(cat_dev::net::parse::not_enough_data))]
	NotEnoughData(&'static str, usize, usize, Bytes),
	/// The overall size of the packet was too long, and there was unexpected
	/// data at the end, a.k.a. the "Trailer".
	#[error("Unexpected Trailer for Packet `{0}` received from the network (we're not sure what do with this extra data), extra bytes: {1:02x?}")]
	#[diagnostic(code(cat_dev::net::parse::unexpected_trailer))]
	UnexpectedTrailer(&'static str, Bytes),
	/// We expected to read UTF-8 data from the network, but it wasn't UTF-8.
	#[error("Data read from the network was expected to be UTF-8, but was not: {0}")]
	#[diagnostic(code(cat_dev::net::parse::utf8_expected))]
	Utf8Expected(#[from] FromUtf8Error),
	/// We expected to read UTF-8 data from the network, but it wasn't UTF-8.
	#[error("Data read from a network slice was expected to be UTF-8, but was not: {0}")]
	#[diagnostic(code(cat_dev::net::parse::utf8_expected_slice))]
	Utf8ExpectedSlice(#[from] Utf8Error),
}
impl From<NetworkParseError> for CatBridgeError {
	fn from(value: NetworkParseError) -> Self {
		Self::Network(value.into())
	}
}
