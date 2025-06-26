//! Error types specifically for interacting with MION CGI's/protos.

use crate::{
	errors::{APIError, CatBridgeError, NetworkError},
	mion::firmware::MIONFirmwareAPIError,
};
use miette::Diagnostic;
use thiserror::Error;

#[cfg(feature = "clients")]
use crate::{
	errors::NetworkParseError,
	mion::{
		cgis::MIONCGIApiError,
		proto::{
			cgis::MIONCGIErrors,
			control::MIONControlProtocolError,
			parameter::{MIONParamProtocolError, MIONParameterAPIError},
		},
	},
};

/// Errors that come from MION APIs specifically.
#[derive(Error, Diagnostic, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MIONAPIError {
	#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
	#[cfg(feature = "clients")]
	#[error(transparent)]
	#[diagnostic(transparent)]
	CGI(#[from] MIONCGIApiError),
	/// You attempted to set the default host bridge to a bridge that does not exist.
	#[error("You cannot set a default bridge that does not exist.")]
	#[diagnostic(code(cat_dev::api::mion::default_device_must_exist))]
	DefaultDeviceMustExist,
	/// You attempted to create a MION device name, but it was empty.
	///
	/// MION Identities must have at LEAST 1 byte.
	#[error("The device name cannot be empty, it must be at least one byte long.")]
	#[diagnostic(code(cat_dev::api::mion::device_name_cannot_be_empty))]
	DeviceNameCannotBeEmpty,
	/// You attempted to create a MION device name, which did not contain ASCII
	/// characters.
	///
	/// MION Identities must contain all ascii characters.
	#[error("A device name has to be completely ASCII! But it wasn't ASCII!")]
	#[diagnostic(code(cat_dev::api::mion::device_name_not_ascii))]
	DeviceNameMustBeAscii,
	/// You attempted to create a MION device name, but it was longer than 255
	/// bytes.
	///
	/// A MION device name has to be serialized into a packet, where it's length
	/// is represented as a [`u8`] which means it can only be [`u8::MAX`], aka
	/// 255 bytes.
	#[error("A Device Name can only be 255 bytes long, but you specified one: {0} bytes long.")]
	#[diagnostic(code(cat_dev::api::mion::device_name_too_long))]
	DeviceNameTooLong(usize),
	/// An error occured handling firmware.
	#[error(transparent)]
	#[diagnostic(transparent)]
	Firmware(#[from] MIONFirmwareAPIError),
	#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
	#[cfg(feature = "clients")]
	#[error(transparent)]
	#[diagnostic(transparent)]
	ParameterSpace(#[from] MIONParameterAPIError),
}

#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
#[cfg(feature = "clients")]
impl From<MIONCGIApiError> for APIError {
	fn from(value: MIONCGIApiError) -> Self {
		Self::MION(value.into())
	}
}
#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
#[cfg(feature = "clients")]
impl From<MIONCGIApiError> for CatBridgeError {
	fn from(value: MIONCGIApiError) -> Self {
		Self::API(value.into())
	}
}

impl From<MIONFirmwareAPIError> for APIError {
	fn from(value: MIONFirmwareAPIError) -> Self {
		Self::MION(value.into())
	}
}
impl From<MIONFirmwareAPIError> for CatBridgeError {
	fn from(value: MIONFirmwareAPIError) -> Self {
		Self::API(value.into())
	}
}

#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
#[cfg(feature = "clients")]
impl From<MIONParameterAPIError> for APIError {
	fn from(value: MIONParameterAPIError) -> Self {
		Self::MION(value.into())
	}
}
#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
#[cfg(feature = "clients")]
impl From<MIONParameterAPIError> for CatBridgeError {
	fn from(value: MIONParameterAPIError) -> Self {
		Self::API(value.into())
	}
}

/// Errors dealing with various MION Protocols.
#[derive(Error, Diagnostic, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MIONProtocolError {
	#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
	#[cfg(feature = "clients")]
	/// Errors related to CGI, and HTML pages.
	#[error(transparent)]
	#[diagnostic(transparent)]
	CGI(#[from] MIONCGIErrors),
	#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
	#[cfg(feature = "clients")]
	/// Errors related to the CONTROL protocol for MION.
	#[error(transparent)]
	#[diagnostic(transparent)]
	Control(#[from] MIONControlProtocolError),
	#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
	#[cfg(feature = "clients")]
	/// Errors related to the PARAMETER SPACE protocol for MION.
	#[error(transparent)]
	#[diagnostic(transparent)]
	Params(#[from] MIONParamProtocolError),
}

impl From<MIONProtocolError> for NetworkError {
	fn from(value: MIONProtocolError) -> Self {
		Self::Parse(value.into())
	}
}
impl From<MIONProtocolError> for CatBridgeError {
	fn from(value: MIONProtocolError) -> Self {
		Self::Network(value.into())
	}
}

#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
#[cfg(feature = "clients")]
impl From<MIONCGIErrors> for NetworkParseError {
	fn from(value: MIONCGIErrors) -> Self {
		Self::MION(value.into())
	}
}
#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
#[cfg(feature = "clients")]
impl From<MIONCGIErrors> for NetworkError {
	fn from(value: MIONCGIErrors) -> Self {
		Self::Parse(value.into())
	}
}
#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
#[cfg(feature = "clients")]
impl From<MIONCGIErrors> for CatBridgeError {
	fn from(value: MIONCGIErrors) -> Self {
		Self::Network(value.into())
	}
}

#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
#[cfg(feature = "clients")]
impl From<MIONParamProtocolError> for NetworkParseError {
	fn from(value: MIONParamProtocolError) -> Self {
		Self::MION(value.into())
	}
}
#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
#[cfg(feature = "clients")]
impl From<MIONParamProtocolError> for NetworkError {
	fn from(value: MIONParamProtocolError) -> Self {
		Self::Parse(value.into())
	}
}
#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
#[cfg(feature = "clients")]
impl From<MIONParamProtocolError> for CatBridgeError {
	fn from(value: MIONParamProtocolError) -> Self {
		Self::Network(value.into())
	}
}

#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
#[cfg(feature = "clients")]
impl From<MIONControlProtocolError> for NetworkParseError {
	fn from(value: MIONControlProtocolError) -> Self {
		Self::MION(value.into())
	}
}
#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
#[cfg(feature = "clients")]
impl From<MIONControlProtocolError> for NetworkError {
	fn from(value: MIONControlProtocolError) -> Self {
		Self::Parse(value.into())
	}
}
#[cfg_attr(docsrs, doc(cfg(feature = "clients")))]
#[cfg(feature = "clients")]
impl From<MIONControlProtocolError> for CatBridgeError {
	fn from(value: MIONControlProtocolError) -> Self {
		Self::Network(value.into())
	}
}
