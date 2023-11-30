//! This module is a wrapper for all the underlying protocol types used for
//! interacting with MIONs.
//!
//! In general as a user you probably don't need to interact with this directly
//! except for maybe importing a few types that APIs return. In general you
//! probably want to use the functions available in the other modules under
//! [`crate::mion`] in order to interact with the MIONs in a safe way.

mod announcement;
pub use announcement::*;

use crate::errors::{NetworkError, NetworkParseError};
use std::fmt::{Display, Formatter, Result as FmtResult};

/// Used as a "Request" & "Response" code for a packet when talking with
/// the MION Bridge.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub enum MionCommandByte {
	Search,
	Broadcast,
	AnnounceYourselves,
	AcknowledgeAnnouncement,
}
impl Display for MionCommandByte {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		match *self {
			Self::Search => write!(fmt, "Search(0x3f)"),
			Self::Broadcast => write!(fmt, "Broadcast(0x21)"),
			Self::AnnounceYourselves => write!(fmt, "AnnounceYourselves(0x2A)"),
			Self::AcknowledgeAnnouncement => write!(fmt, "AcknowledgeAnnouncement(0x20)"),
		}
	}
}
impl TryFrom<u8> for MionCommandByte {
	type Error = NetworkError;

	fn try_from(value: u8) -> Result<Self, Self::Error> {
		match value {
			0x3F => Ok(Self::Search),
			0x21 => Ok(Self::Broadcast),
			0x2A => Ok(Self::AnnounceYourselves),
			0x20 => Ok(Self::AcknowledgeAnnouncement),
			_ => Err(NetworkError::ParseError(NetworkParseError::UnknownCommand(
				value,
			))),
		}
	}
}
impl From<MionCommandByte> for u8 {
	fn from(value: MionCommandByte) -> Self {
		match value {
			MionCommandByte::Search => 0x3F,
			MionCommandByte::Broadcast => 0x21,
			MionCommandByte::AnnounceYourselves => 0x2A,
			MionCommandByte::AcknowledgeAnnouncement => 0x20,
		}
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[test]
	pub fn ser_and_deser() {
		for command_byte in vec![
			MionCommandByte::Search,
			MionCommandByte::Broadcast,
			MionCommandByte::AnnounceYourselves,
			MionCommandByte::AcknowledgeAnnouncement,
		] {
			assert_eq!(
				command_byte,
				MionCommandByte::try_from(u8::from(command_byte))
					.expect("Failed to serialize/deserialize command byte: {command_byte}"),
				"MionCommandByte was not the same after serializing, and deserializing."
			);
		}
	}
}
