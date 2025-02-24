//! Errors for `PCFS`, and `FSEmul`.

use crate::{
	errors::{APIError, CatBridgeError, NetworkError, NetworkParseError},
	fsemul::{
		atapi::errors::ATAPIProtocolError,
		pcfs::errors::{PCFSApiError, SataProtocolError},
		sdio::errors::{SDIOAPIError, SDIOProtocolError},
	},
};
use bytes::Bytes;
use miette::Diagnostic;
use std::path::PathBuf;
use thiserror::Error;

/// Errors related to API errors for `FSEmul`.
#[derive(Diagnostic, Error, Debug, PartialEq, Eq)]
pub enum FSEmulAPIError {
	/// This DLF address is too large to be inserted.
	#[error("DLF Address is too large ({0:016X}) to be inserted ({1:016X})")]
	#[diagnostic(code(cat_dev::api::fsemul::dlf_address_too_large))]
	DlfAddressTooLarge(u128, u128),
	#[error("DLF Paths _MUST_ be able tto be encoded as UTF-8, this file path was not: {0:02X?}")]
	#[diagnostic(code(cat_dev::api::fsemul::dlf_path_forced_utf8))]
	DlfPathMustBeUtf8(Bytes),
	/// DLF files do mandate that we have the end of a file which is represented
	/// as the last address in the file.
	#[error("DLF files must have an ending address that appears _last_, and is an empty string.")]
	#[diagnostic(code(cat_dev::api::fsemul::dlf_must_have_ending))]
	DlfMustHaveEnding,
	#[error(
		"You tried to place a disk item past the current ending, please update the ending, before updating the new item."
	)]
	#[diagnostic(code(cat_dev::api::fsemul::dlf_update_ending_first))]
	DlfUpsertEndingFirst,
	#[error("Failed to interact with path, file must be open first: {0:?}")]
	#[diagnostic(code(cat_dev::api::fsemul::path_not_open))]
	PathNotOpen(PathBuf),
	#[error(transparent)]
	#[diagnostic(transparent)]
	PCFS(#[from] PCFSApiError),
	#[error(transparent)]
	#[diagnostic(transparent)]
	SDIO(#[from] SDIOAPIError),
}

impl From<FSEmulAPIError> for CatBridgeError {
	fn from(value: FSEmulAPIError) -> Self {
		Self::API(value.into())
	}
}

impl From<PCFSApiError> for APIError {
	fn from(value: PCFSApiError) -> Self {
		Self::FSEmul(value.into())
	}
}
impl From<PCFSApiError> for CatBridgeError {
	fn from(value: PCFSApiError) -> Self {
		Self::API(value.into())
	}
}

impl From<SDIOAPIError> for APIError {
	fn from(value: SDIOAPIError) -> Self {
		Self::FSEmul(value.into())
	}
}
impl From<SDIOAPIError> for CatBridgeError {
	fn from(value: SDIOAPIError) -> Self {
		Self::API(value.into())
	}
}

/// Errors dealing with the filesystem specifically related to
#[derive(Diagnostic, Error, Debug, PartialEq, Eq)]
pub enum FSEmulFSError {
	/// We cannot find a path to store our `FSEmul` configuration.
	#[error(
		"We can't find the path to store fsemul configuration, please use explicit paths instead."
	)]
	#[diagnostic(code(cat_dev::fs::fsemul::cant_find_path))]
	CantFindPath,
	/// We cannot find the root `CAFE_SDK` path.
	#[error("We can't find the root Cafe SDK directory, please use explicit paths instead.")]
	#[diagnostic(code(cat_dev::fs::fsemul::cant_find_cafe_sdk_path))]
	CantFindCafeSdkPath,
	/// The passed in Cafe SDK path did not have the appropriate directories.
	#[error("The Cafe SDK Path does not have the appropriate MLC path directories.")]
	#[diagnostic(code(cat_dev::fs::fsemul::corrupt_cafe_sdk_path))]
	CafeSdkPathCorrupt,
	/// A DLF file contained a very invalid line.
	#[error(
		"While parsing a disk layout file we ran into a line which is not in the format of: `<hex address>,\"<path>\"`: {0}"
	)]
	#[diagnostic(code(cat_dev::fs::fsemul::corrupt_dlf_line))]
	DlfCorruptLine(String),
	/// A DLF file contained a bad termination line.
	#[error("DLF files must have a blank final line, but was: {0}")]
	#[diagnostic(code(cat_dev::fs::fsemul::corrupt_dlf_final_line))]
	DlfCorruptFinalLine(String),
	/// A DLF file had a bad version string file.
	#[error(
		"While parsing a disk layout file, the first line should be a version string (e.g. `v1.00`), which this was not: {0}"
	)]
	#[diagnostic(code(cat_dev::fs::fsemul::corrupt_dlf_version_line))]
	DlfCorruptVersionLine(String),
}
impl From<FSEmulFSError> for CatBridgeError {
	fn from(value: FSEmulFSError) -> Self {
		Self::FS(value.into())
	}
}

/// Errors related to protocols for `FSEmul`.
#[derive(Diagnostic, Error, Debug, PartialEq, Eq)]
pub enum FSEmulProtocolError {
	#[error(transparent)]
	#[diagnostic(transparent)]
	ATAPI(#[from] ATAPIProtocolError),
	#[error(transparent)]
	#[diagnostic(transparent)]
	PCFSSata(#[from] SataProtocolError),
	#[error(transparent)]
	#[diagnostic(transparent)]
	SDIO(#[from] SDIOProtocolError),
}

impl From<FSEmulProtocolError> for NetworkError {
	fn from(value: FSEmulProtocolError) -> Self {
		Self::Parse(value.into())
	}
}
impl From<FSEmulProtocolError> for CatBridgeError {
	fn from(value: FSEmulProtocolError) -> Self {
		Self::Network(value.into())
	}
}

impl From<ATAPIProtocolError> for NetworkParseError {
	fn from(value: ATAPIProtocolError) -> Self {
		Self::FSEmul(value.into())
	}
}
impl From<ATAPIProtocolError> for NetworkError {
	fn from(value: ATAPIProtocolError) -> Self {
		Self::Parse(value.into())
	}
}
impl From<ATAPIProtocolError> for CatBridgeError {
	fn from(value: ATAPIProtocolError) -> Self {
		Self::Network(value.into())
	}
}

impl From<SataProtocolError> for NetworkParseError {
	fn from(value: SataProtocolError) -> Self {
		Self::FSEmul(value.into())
	}
}
impl From<SataProtocolError> for NetworkError {
	fn from(value: SataProtocolError) -> Self {
		Self::Parse(value.into())
	}
}
impl From<SataProtocolError> for CatBridgeError {
	fn from(value: SataProtocolError) -> Self {
		Self::Network(value.into())
	}
}

impl From<SDIOProtocolError> for NetworkParseError {
	fn from(value: SDIOProtocolError) -> Self {
		Self::FSEmul(value.into())
	}
}
impl From<SDIOProtocolError> for NetworkError {
	fn from(value: SDIOProtocolError) -> Self {
		Self::Parse(value.into())
	}
}
impl From<SDIOProtocolError> for CatBridgeError {
	fn from(value: SDIOProtocolError) -> Self {
		Self::Network(value.into())
	}
}
