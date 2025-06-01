//! Error types specifically for clients in CAT-DEV.

use miette::{Diagnostic, Report};
use thiserror::Error;

#[derive(Diagnostic, Error, Debug)]
pub enum CommonNetClientNetworkError {
	#[error("This client tried to send/receive packets but was not connected to a server.")]
	#[diagnostic(code(cat_dev::net::client::not_connected_to_server))]
	NotConnectedToServer,
	#[error("This client tried to serialize a packet but failed: {0:?}")]
	#[diagnostic(code(cat_dev::net::client::serialization_error))]
	SerializationError(Report),
	#[error("The client tried to queue up a packet to send but failed: {0:?}")]
	#[diagnostic(code(cat_dev::net::client::cannot_queue_send))]
	CannotQueueSend(String),
}
