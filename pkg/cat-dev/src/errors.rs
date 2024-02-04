//! A container for all the types of errors generated crate-wide.
//!
//! The top level error type is: [`CatBridgeError`], which wraps all the other
//! types of errors. You can find more specific error types documented on each
//! specific item.

use bytes::Bytes;
use miette::Diagnostic;
use std::string::FromUtf8Error;
use thiserror::Error;
use tokio::{io::Error as IoError, task::JoinError};

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
}

/// An error that comes from one of our APIs, e.g. passing in a parameter
/// that wasn't expected.
///
/// All the APIs within this crate will have errors will be collapsed under
/// this particular error type. There will be no inner separation between
/// modules.
#[derive(Error, Diagnostic, Debug, PartialEq, Eq)]
pub enum APIError {
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
	#[diagnostic(code(cat_dev::fs::cant_find_path))]
	CantFindHostEnvPath,
	/// We expected to read UTF-8 data from the filesystem, but it wasn't UTF-8.
	#[error("Data read from the filesystem was expected to be UTF-8, but was not: {0}")]
	#[diagnostic(code(cat_dev::fs::utf8_expected))]
	InvalidDataNeedsUTF8(#[from] FromUtf8Error),
	/// We expected to parse file as an INI data.
	#[error("Data read from the filesystem was expected to be a valid INI file: {0}")]
	#[diagnostic(code(cat_dev::fs::expected_ini))]
	InvalidDataNeedsToBeINI(String),
	/// See [`tokio::io::Error`] for details.
	#[error("Error writing/reading data from the filesystem: {0}")]
	#[diagnostic(code(cat_dev::fs::io_failure))]
	IOError(#[from] IoError),
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
	///
	/// We just end up formatting this to a string so we can continue to derive
	/// [`PartialEq`], and [`Eq`].
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
	#[error(
		"Timed out while writing/reading data from the network, failed to send and receive data."
	)]
	#[diagnostic(code(cat_dev::net::timeout))]
	TimeoutError,
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
	/// Unknown packet tpe for the MION Params port.
	#[error("Unknown Packet Type: `{0}` received from the network (this may mean your CAT-DEV is doing something we didn't expect)")]
	#[diagnostic(code(cat_dev::net::parse::params::unknown_packet_type))]
	UnknownParamsPacketType(i32),
	#[error("Error code received from MION Params: `{0}`")]
	#[diagnostic(code(cat_dev::net::parse::params::error_code))]
	ParamsPacketErrorCode(i32),
}
