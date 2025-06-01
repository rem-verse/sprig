//! Common models for all L4 Services.
//!
//! This mostly includes the [`Request`], and [`Response`] structures that
//! services are actively passed, and expected to return.

use crate::{
	errors::CatBridgeError,
	net::{Extensions, errors::CommonNetAPIError},
};
use bytes::{Bytes, BytesMut};
use fnv::FnvHasher;
use futures::Future;
use std::{
	fmt::{Debug, Formatter, Result as FmtResult},
	hash::{Hash, Hasher},
	marker::Send,
	net::SocketAddr,
};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};

#[cfg(feature = "servers")]
use crate::{errors::NetworkError, net::errors::CommonNetNetworkError};
#[cfg(feature = "servers")]
use std::sync::Arc;
#[cfg(feature = "servers")]
use tokio::{io::AsyncReadExt, net::TcpStream, sync::Mutex};

/// Used to do reference-to-value conversions thus not consuming the input value.
///
/// This is mainly used with state's to extract "substates" from a reference to main application
/// state.
pub trait FromRef<InputTy> {
	/// Converts to this type from a reference to the input type.
	fn from_ref(input: &InputTy) -> Self;
}

impl<InnerTy> FromRef<InnerTy> for InnerTy
where
	InnerTy: Clone,
{
	fn from_ref(input: &InnerTy) -> Self {
		input.clone()
	}
}

/// A request that came from either a TCP/UDP source.
pub struct Request<State: Clone + Send + Sync + 'static> {
	/// The actual body of the the underlying request.
	body: Bytes,
	/// Extensions that can in particular be attached to this request.
	ext: Extensions,
	/// The source address of where the request came from.
	source_address: SocketAddr,
	/// The active state for this request.
	state: State,
	/// The stream ID this request came in on.
	stream_id: Option<u64>,
	/// Indicate that the response needs to read a certain size, ignoring
	/// whatever the current NAGLE algorithim says.
	///
	/// This will still call 'post nagle hook', 'trace io', and will still
	/// obey NAGLE timeouts. It just overrides the _kind_ of NAGLE we do.
	#[cfg(feature = "clients")]
	explicit_read_amount: Option<usize>,
	/// Allow accessing the raw underlying stream while processing the request.
	#[cfg(feature = "servers")]
	stream_access: Option<Arc<Mutex<Option<TcpStream>>>>,
}

impl<State: Clone + Send + Sync + 'static> Request<State>
where
	State: Default,
{
	#[must_use]
	pub fn new(body: Bytes, source_address: SocketAddr, stream_id: Option<u64>) -> Self {
		Self {
			body,
			ext: Extensions::new(),
			source_address,
			state: Default::default(),
			stream_id,
			#[cfg(feature = "clients")]
			explicit_read_amount: None,
			#[cfg(feature = "servers")]
			stream_access: None,
		}
	}
}

impl<State: Clone + Send + Sync + 'static> Request<State> {
	#[must_use]
	pub fn new_with_state(
		body: Bytes,
		source_address: SocketAddr,
		state: State,
		stream_id: Option<u64>,
	) -> Self {
		Self {
			body,
			ext: Extensions::new(),
			source_address,
			state,
			stream_id,
			#[cfg(feature = "clients")]
			explicit_read_amount: None,
			#[cfg(feature = "servers")]
			stream_access: None,
		}
	}

	#[cfg(feature = "clients")]
	#[must_use]
	pub fn new_with_state_and_read_amount(
		body: Bytes,
		source_address: SocketAddr,
		state: State,
		stream_id: Option<u64>,
		explicit_read_amount: usize,
	) -> Self {
		Self {
			body,
			ext: Extensions::new(),
			source_address,
			state,
			stream_id,
			explicit_read_amount: Some(explicit_read_amount),
			#[cfg(feature = "servers")]
			stream_access: None,
		}
	}

	#[cfg(feature = "servers")]
	#[must_use]
	pub fn new_with_state_and_stream(
		body: Bytes,
		source_address: SocketAddr,
		state: State,
		stream_id: Option<u64>,
		stream: Arc<Mutex<Option<TcpStream>>>,
	) -> Self {
		Self {
			body,
			ext: Extensions::new(),
			source_address,
			state,
			stream_id,
			#[cfg(feature = "clients")]
			explicit_read_amount: None,
			stream_access: Some(stream),
		}
	}

	/// Swap the body of the request to something new.
	pub fn swap_body(&mut self, new_body: Bytes) {
		self.body = new_body;
	}

	/// Update the core request source.
	pub const fn update_request_source(&mut self, source: SocketAddr, stream_id: Option<u64>) {
		self.source_address = source;
		self.stream_id = stream_id;
	}

	/// A unique identifier for the "stream" or connection of a packet.
	///
	/// In UDP which doesn't have stream this uses the source address as
	/// the core identifier.
	#[must_use]
	pub fn stream_id(&self) -> u64 {
		if let Some(id) = self.stream_id {
			id
		} else {
			let mut hasher = FnvHasher::default();
			self.source_address.hash(&mut hasher);
			hasher.finish()
		}
	}

	/// A client has requested we send this request, and then read an explicit
	/// amount of bytes, ignoring whatever the current NAGLE method is.
	///
	/// This is a utility only available when we are a client, and are receiving
	/// a packet that changes what our nagle split is for it's specific response
	/// while keeping the nalge the same otherwise.
	#[cfg(feature = "clients")]
	#[must_use]
	pub const fn explicit_read_amount(&self) -> Option<usize> {
		self.explicit_read_amount
	}

	/// Override the current NAGLE algorithm being used by this client for this
	/// single request/response pair. Do a single non-nagle'd receive.
	#[cfg(feature = "clients")]
	pub const fn set_explicit_read_amount(&mut self, new_read_amount: usize) {
		self.explicit_read_amount = Some(new_read_amount);
	}

	/// Attempt to read more bytes from the TCP Stream directly.
	///
	/// This is a utility only available when we are a server, and need to request
	/// more info from the client.
	///
	/// ***THIS WILL BYPASS EVERYYTHING PROVIDED BY TCP SERVER, AND JUST READ RAW
	/// BYTES FROM THE STREAM.*** This is only for requests like File I/O which
	/// _need_ to bypass all the logic provided by the stream classes.
	///
	/// ## Errors
	///
	/// If the request has been moved outside of it's original processing place,
	/// and it is no longer possible to read from the stream.
	#[cfg(feature = "servers")]
	pub async fn unsafe_read_more_bytes_from_stream(
		&self,
		to_read: usize,
	) -> Result<Bytes, CatBridgeError> {
		if let Some(strm) = self.stream_access.as_ref() {
			let mut guard = strm.lock().await;

			if let Some(stream) = guard.as_mut() {
				let mut buff = BytesMut::with_capacity(to_read);
				stream.readable().await.map_err(NetworkError::IO)?;
				let mut needed = to_read;
				while needed > 0 {
					let read = stream.read_buf(&mut buff).await.map_err(NetworkError::IO)?;
					needed -= read;
				}
				return Ok::<Bytes, CatBridgeError>(buff.freeze());
			}
		}

		Err(CommonNetNetworkError::StreamNoLongerProcessing.into())
	}

	#[must_use]
	pub const fn body(&self) -> &Bytes {
		&self.body
	}
	#[must_use]
	pub fn body_mut(&mut self) -> &mut Bytes {
		&mut self.body
	}
	pub fn set_body(&mut self, new_body: Bytes) {
		self.body = new_body;
	}
	#[must_use]
	pub fn body_owned(self) -> Bytes {
		self.body
	}

	#[must_use]
	pub const fn extensions(&self) -> &Extensions {
		&self.ext
	}
	#[must_use]
	pub fn extensions_mut(&mut self) -> &mut Extensions {
		&mut self.ext
	}
	#[must_use]
	pub fn extensions_owned(self) -> Extensions {
		self.ext
	}

	#[must_use]
	pub const fn state(&self) -> &State {
		&self.state
	}
	#[must_use]
	pub fn state_mut(&mut self) -> &mut State {
		&mut self.state
	}

	#[must_use]
	pub const fn source(&self) -> &SocketAddr {
		&self.source_address
	}
	#[must_use]
	pub fn is_ipv4(&self) -> bool {
		self.source_address.ip().is_ipv4()
	}
	#[must_use]
	pub fn is_ipv6(&self) -> bool {
		self.source_address.ip().is_ipv6()
	}
}

impl<State: Clone + Send + Sync + 'static> Clone for Request<State> {
	fn clone(&self) -> Self {
		Request {
			body: self.body.clone(),
			ext: Extensions::new(),
			source_address: self.source_address,
			state: self.state.clone(),
			stream_id: self.stream_id,
			#[cfg(feature = "clients")]
			explicit_read_amount: self.explicit_read_amount,
			#[cfg(feature = "servers")]
			stream_access: self.stream_access.clone(),
		}
	}
}

impl<State: Clone + Send + Sync + 'static> Debug for Request<State>
where
	State: Debug,
{
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		let mut dbg_struct = fmt.debug_struct("Request");

		dbg_struct
			.field("body", &self.body)
			// Extensions can't be printed in debug by hyper, and in order to keep
			// compatability ours don't.
			.field("source_address", &self.source_address)
			.field("stream_id", &self.stream_id);

		#[cfg(feature = "clients")]
		dbg_struct.field("explicit_read_amount", &self.explicit_read_amount);
		#[cfg(feature = "servers")]
		dbg_struct.field(
			"stream_access",
			&if self.stream_access.is_some() {
				"<stream>"
			} else {
				"<none>"
			},
		);

		dbg_struct.finish_non_exhaustive()
	}
}

const REQUEST_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("body"),
	NamedField::new("source_address"),
	NamedField::new("stream_id"),
	#[cfg(feature = "clients")]
	NamedField::new("explicit_read_amount"),
	#[cfg(feature = "servers")]
	NamedField::new("stream_access"),
];

impl<State: Clone + Send + Sync + 'static> Structable for Request<State> {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static("Request", Fields::Named(REQUEST_FIELDS))
	}
}

impl<State: Clone + Send + Sync + 'static> Valuable for Request<State> {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			REQUEST_FIELDS,
			&[
				Valuable::as_value(&format!("{:02X?}", self.body)),
				Valuable::as_value(&format!("{}", self.source_address)),
				Valuable::as_value(&self.stream_id),
				#[cfg(feature = "clients")]
				Valuable::as_value(&self.explicit_read_amount),
				#[cfg(feature = "servers")]
				Valuable::as_value(&if self.stream_access.is_some() {
					"<stream>"
				} else {
					"<none>"
				}),
			],
		));
	}
}

/// Just a generic response on an L4 Layer.
#[derive(Clone, Debug)]
pub struct Response {
	/// Get the body of the actual response to send. If empty, no response is sent.
	pub body: Option<Bytes>,
	/// If we should request any long-lived connections should be closed.
	///
	/// NOTE: not every type of Level 4 connection has a long lived connection,
	/// or stream. UDP is a prime example of this, this is not guaranteed.
	pub request_connection_close: bool,
}

impl Response {
	#[must_use]
	pub const fn new_empty() -> Self {
		Self {
			body: None,
			request_connection_close: false,
		}
	}
	#[must_use]
	pub const fn empty_close() -> Self {
		Self {
			body: None,
			request_connection_close: true,
		}
	}
	#[must_use]
	pub const fn new_with_body(body: Bytes) -> Self {
		Self {
			body: Some(body),
			request_connection_close: false,
		}
	}

	#[must_use]
	pub const fn body(&self) -> Option<&Bytes> {
		self.body.as_ref()
	}
	#[must_use]
	pub fn body_mut(&mut self) -> Option<&mut Bytes> {
		self.body.as_mut()
	}
	pub fn set_body(&mut self, bytes: Bytes) {
		self.body = Some(bytes);
	}
	#[must_use]
	pub fn take_body(self) -> Option<Bytes> {
		self.body
	}

	#[must_use]
	pub const fn request_connection_close(&self) -> bool {
		self.request_connection_close
	}
	pub fn should_close_connection(&mut self) {
		self.request_connection_close = true;
	}
	pub fn dont_close_connection(&mut self) {
		self.request_connection_close = false;
	}
}

impl Default for Response {
	fn default() -> Self {
		Self::new_empty()
	}
}

impl<ByteTy: Into<Bytes>> From<ByteTy> for Response {
	fn from(resp: ByteTy) -> Self {
		Self::new_with_body(resp.into())
	}
}

const RESPONSE_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("body"),
	NamedField::new("request_connection_close"),
];

impl Structable for Response {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static("Response", Fields::Named(RESPONSE_FIELDS))
	}
}

impl Valuable for Response {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			RESPONSE_FIELDS,
			&[
				Valuable::as_value(&if let Some(body_ref) = self.body.as_ref() {
					format!("{body_ref:02X?}")
				} else {
					"<empty>".to_owned()
				}),
				Valuable::as_value(&self.request_connection_close),
			],
		));
	}
}

/// Extract any value from a Request, allowing more people to keep using it.
///
/// Kept as our own trait so it can be async like axum.
pub trait FromRequestParts<State: Clone + Send + Sync + 'static>: Sized {
	fn from_request_parts(
		req: &mut Request<State>,
	) -> impl Future<Output = Result<Self, CatBridgeError>> + Send;
}

/// Extract any value from a Request, finalizing it.
///
/// Kept as our own trait so it can be async like axum.
pub trait FromRequest<State: Clone + Send + Sync + 'static>: Sized {
	fn from_request(
		req: Request<State>,
	) -> impl Future<Output = Result<Self, CatBridgeError>> + Send;
}
impl<State: Clone + Send + Sync + 'static> FromRequest<State> for Request<State> {
	async fn from_request(req: Request<State>) -> Result<Self, CatBridgeError> {
		Ok(req)
	}
}

/// A blanket trait to implement into a full response.
///
/// This was mainly implemented so functions that return things like
/// `Bytes`, can naturally get wrapped into a result without needing
/// to return a result themselves.
pub trait IntoResponse: Sized {
	/// Convert an arbitrary type to a Response.
	///
	/// # Errors
	///
	/// If for whatever reason the type can't be turned into a response.
	fn to_response(self) -> Result<Response, CatBridgeError>;
}

impl IntoResponse for () {
	fn to_response(self) -> Result<Response, CatBridgeError> {
		Ok(Response::new_empty())
	}
}
impl IntoResponse for Response {
	fn to_response(self) -> Result<Response, CatBridgeError> {
		Ok(self)
	}
}

macro_rules! impl_from_ok_response {
	($ty:ty) => {
		impl IntoResponse for $ty {
			fn to_response(self) -> Result<Response, CatBridgeError> {
				Ok(self.into())
			}
		}
	};
}
impl_from_ok_response!(Bytes);
impl_from_ok_response!(BytesMut);
impl_from_ok_response!(String);
impl_from_ok_response!(Vec<u8>);
impl_from_ok_response!(&'static [u8]);
impl_from_ok_response!(&'static str);

impl IntoResponse for CatBridgeError {
	fn to_response(self) -> Result<Response, CatBridgeError> {
		Err(self)
	}
}

impl<SomeTy: IntoResponse> IntoResponse for Option<SomeTy> {
	fn to_response(self) -> Result<Response, CatBridgeError> {
		if let Some(val) = self {
			val.to_response()
		} else {
			Ok(Response::new_empty())
		}
	}
}
impl<OkTy: IntoResponse> IntoResponse for Result<OkTy, CatBridgeError> {
	fn to_response(self) -> Result<Response, CatBridgeError> {
		self.and_then(IntoResponse::to_response)
	}
}

/// Nagle guard is what determines when a packet "begins", and "ends".
///
/// These are the various types of ways we determine where a packet begins,
/// and "ends".
#[derive(Clone, Debug, PartialEq, Eq, Hash, Valuable)]
pub enum NagleGuard {
	/// Search for a specific searchs of bytes to determine the "end" of a
	/// packet.
	EndSigilSearch(&'static [u8]),
	/// All packets are guaranteed to be the exact same length.
	StaticSize(usize),
	/// Packets will prefix their total length with a u16.
	///
	/// This includes the 'endianness' to parse the number as, and you can apply
	/// an extra length to add (incase the length doesn't say include the length
	/// of a header).
	U16LengthPrefixed(Endianness, Option<usize>),
	/// Packets will prefix their total length with a u32.
	///
	/// This includes the 'endianness' to parse the number as, and you can apply
	/// an extra length to add (incase the length doesn't say include the length
	/// of a header).
	U32LengthPrefixed(Endianness, Option<usize>),
}

impl NagleGuard {
	/// Split a buffer of bytes from potentially multiple packets.
	///
	/// ## Errors
	///
	/// If we are an "end sigil search" without a an actual sigil.
	pub fn split(&self, buff: &BytesMut) -> Result<Option<(usize, usize)>, CommonNetAPIError> {
		match *self {
			NagleGuard::EndSigilSearch(sigil) => {
				if sigil.is_empty() {
					return Err(CommonNetAPIError::NagleGuardEndSigilCannotBeEmpty);
				}
				if buff.is_empty() {
					return Ok(None);
				}

				for (idx, byte) in buff.iter().enumerate() {
					// Not enough room!
					if idx + sigil.len() > buff.len() {
						break;
					}
					if *byte == sigil[0] && sigil == &buff[idx..(idx + sigil.len())] {
						return Ok(Some((0, idx + sigil.len())));
					}
				}
			}
			NagleGuard::StaticSize(size) => {
				if buff.len() < size {
					return Ok(None);
				}

				return Ok(Some((0, size)));
			}
			NagleGuard::U16LengthPrefixed(endianness, extra_len) => {
				if buff.len() < 2 {
					return Ok(None);
				}
				let extra_len_frd = extra_len.unwrap_or_default();

				let total_size = match endianness {
					Endianness::Little => u16::from_le_bytes([buff[0], buff[1]]),
					Endianness::Big => u16::from_be_bytes([buff[0], buff[1]]),
				};
				if buff.len() >= usize::from(total_size) + extra_len_frd {
					return Ok(Some((0, usize::from(total_size) + extra_len_frd)));
				}
			}
			NagleGuard::U32LengthPrefixed(endianness, extra_len) => {
				if buff.len() < 4 {
					return Ok(None);
				}
				let extra_len_frd = extra_len.unwrap_or_default();

				let total_size = match endianness {
					Endianness::Little => u32::from_le_bytes([buff[0], buff[1], buff[2], buff[3]]),
					Endianness::Big => u32::from_be_bytes([buff[0], buff[1], buff[2], buff[3]]),
				};
				if buff.len() >= usize::try_from(total_size).unwrap_or(usize::MAX) + extra_len_frd {
					return Ok(Some((
						0,
						usize::try_from(total_size).unwrap_or(usize::MAX) + extra_len_frd,
					)));
				}
			}
		}

		Ok(None)
	}
}

impl From<usize> for NagleGuard {
	fn from(value: usize) -> Self {
		NagleGuard::StaticSize(value)
	}
}

impl From<&'static [u8]> for NagleGuard {
	fn from(value: &'static [u8]) -> Self {
		NagleGuard::EndSigilSearch(value)
	}
}

/// The endianness of a particular number coming in over the network.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Valuable)]
pub enum Endianness {
	/// The data is in little endian.
	Little,
	/// The data is in big endian.
	Big,
}

/// A function type that can be used to convert before passing onto nagle.
///
/// This is useful when we have an encrypted stream that needs to be decrypted,
/// before we end up applying any NAGLE, or other splitting logic to the
/// stream.
pub trait PreNagleFnTy: Fn(u64, &mut BytesMut) + Send + Sync + 'static {}
impl<FnTy: Fn(u64, &mut BytesMut) + Send + Sync + 'static> PreNagleFnTy for FnTy {}

/// A function type that can be used to convert data right before sending it
/// out.
///
/// This is useful when we have an encrypted stream that needs to be encrypted
/// before it goes out to the client.
pub trait PostNagleFnTy: Fn(u64, Bytes) -> Bytes + Send + Sync + 'static {}
impl<FnTy: Fn(u64, Bytes) -> Bytes + Send + Sync + 'static> PostNagleFnTy for FnTy {}
