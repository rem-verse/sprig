//! Error types specifically related to common network utilities.

use miette::Diagnostic;
use std::{net::SocketAddr, time::Duration};
use thiserror::Error;
use tokio::io::Error as TokioIOError;

/// API related errors to the common network stack.
#[derive(Error, Diagnostic, Debug)]
pub enum CommonNetAPIError {
	#[error("Failed to lookup host to bind too (TCP): {0:?}")]
	#[diagnostic(code(cat_dev::net::api::address_lookup_error))]
	AddressLookupError(#[from] TokioIOError),
	#[error("You cannot register two fallback handlers on the same router!")]
	#[diagnostic(code(cat_dev::net::api::cannot_register_two_fallback_handlers))]
	DuplicateFallbackHandler,
	#[error("Built-in L4 router does not support multiple handlers on the same route!")]
	#[diagnostic(code(cat_dev::net::api::duplicate_route))]
	DuplicateRoute(Vec<u8>),
	#[error("Failed to find an extension that was requested for a handler!")]
	#[diagnostic(code(cat_dev::net::api::extension_not_present))]
	ExtensionNotPresent,
	#[error("A Nagle guard on an end search cannot be empty!")]
	#[diagnostic(code(cat_dev::net::api::nagle_guard_end_sigil_cannot_be_empty))]
	NagleGuardEndSigilCannotBeEmpty,
	#[error("A stream begin hook or (on connection hook) has already been registered!")]
	#[diagnostic(code(cat_dev::net::api::stream_begin_hook_already_registered))]
	OnStreamBeginAlreadyRegistered,
	#[error(
		"A stream begin hook or (on connection hook) has not yet been registered! It is needed to layer!"
	)]
	#[diagnostic(code(cat_dev::net::api::stream_begin_hook_not_registered))]
	OnStreamBeginNotRegistered,
	#[error("A stream end hook or (on disconnect hook) has already been registered!")]
	#[diagnostic(code(cat_dev::net::api::stream_end_hook_already_registered))]
	OnStreamEndAlreadyRegistered,
	#[error(
		"A stream end hook or (on disconnect hook) has not yet been registered! It is needed to layer!"
	)]
	#[diagnostic(code(cat_dev::net::api::stream_begin_end_not_registered))]
	OnStreamEndNotRegistered,
	#[error(
		"The built-in router for our L4 server had `add_route` called without a byte prefix to match on. We need at least one byte!"
	)]
	#[diagnostic(code(cat_dev::net::api::router_needs_some_bytes))]
	RouterNeedsSomeBytesToMatchOn,
	#[error(
		"The built-in router for our L4 server had `add_route` called with a series of bytes that are longer than 16 bytes: {0:02x?}, but we can only match on 16 bytes long!"
	)]
	#[diagnostic(code(cat_dev::net::api::router_too_long_to_match_on))]
	RouteTooLongToMatchOn(Vec<u8>),
	#[error(
		"Looking up address returned no/multiple address to bind too, but we only know how to bind to a single address: {0:?}"
	)]
	#[diagnostic(code(cat_dev::net::api::wrong_amount_of_addresses_to_bind_too))]
	WrongAmountOfAddressesToBindToo(Vec<SocketAddr>),
}

#[derive(Error, Diagnostic, Debug, PartialEq, Eq)]
pub enum CommonNetNetworkError {
	/// A connection has taken us too long to send us a single packet.
	#[error("A client has taken too long to send us a single packet, it has been: {0:?}")]
	#[diagnostic(code(cat_dev::net::common_network::slowloris_timeout))]
	SlowlorisTimeout(Duration),
	/// Stream is no longer actively processing a request.
	#[error(
		"internal: A request tried to read more data from it's stream, but the stream is not actively being processed!"
	)]
	#[diagnostic(code(cat_dev::net::common_network::stream_no_longer_processing))]
	StreamNoLongerProcessing,
}
