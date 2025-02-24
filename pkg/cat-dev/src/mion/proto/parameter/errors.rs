//! Errors related to MION Parameter Space Port.

use miette::Diagnostic;
use thiserror::Error;

#[derive(Error, Diagnostic, Debug, PartialEq, Eq)]
pub enum MIONParameterAPIError {
	/// You passed a parameter space to an API that requires the full parameter
	/// space, but it was not the correct length (512 bytes).
	#[error(
		"The MION Parameter body you passed in was: {0} bytes long, but must be exactly 512 bytes long!"
	)]
	#[diagnostic(code(cat_dev::api::mion::parameter::body_incorrect_length))]
	BodyNotCorrectLength(usize),
	/// You tried asking for a parameter of a specific name, but we could not
	/// find a parameter with the name you specified.
	///
	/// We have created the concept of "name"'s for some parameters in the
	/// parameter space. Although the official CLI tools just used indexes, I
	/// in particular find indexes hard to remember so wanted to ensure folks
	/// could just "say" what they wanted to lookup. Of course though not every
	/// field is named, nor does it mean the API was given a non typo'd value.
	#[error("The MION Parameter name: {0} is not known, cannot find index.")]
	#[diagnostic(code(cat_dev::api::mion::parameter::name_not_known))]
	NameNotKnown(String),
	/// You tried asking for a parameter that does not exist.
	///
	/// There are only 512 parameters, so you can only ask for parameters in
	/// (0-511) inclusive.
	#[error(
		"You asked for the MION Parameter at index: {0}, but MION Parameter indexes cannot be greater than 511."
	)]
	#[diagnostic(code(cat_dev::api::mion::parameter::not_in_range))]
	NotInRange(usize),
}

#[derive(Error, Diagnostic, Debug, PartialEq, Eq)]
pub enum MIONParamProtocolError {
	/// We got an error code back from trying to interact with the MION
	/// paramspace port.
	///
	/// We unfortunately do not have these error codes known at this point in
	/// time.
	#[error("Error code received from MION Params: `{0}`")]
	#[diagnostic(code(cat_dev::net::parse::mion::params::error_code))]
	ErrorCode(i32),
	/// Unknown packet type for the MION Params port.
	#[error(
		"Unknown Packet Type: `{0}` received from the network (this may mean your CAT-DEV is doing something we didn't expect)"
	)]
	#[diagnostic(code(cat_dev::net::parse::mion::params::unknown_packet_type))]
	PacketType(i32),
}
