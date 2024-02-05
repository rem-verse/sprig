//! Get parameters from the parameter space of a MION.

use crate::{
	errors::{APIError, CatBridgeError, NetworkError, NetworkParseError},
	mion::proto::{
		parameter::{
			well_known::{index_from_parameter_name, ParameterLocationSpecification},
			DumpedMionParameters, MionDumpParameters, SetMionParameters, SetMionParametersResponse,
		},
		MION_PARAMETER_PORT, MION_PARAMETER_TIMEOUT_SECONDS,
	},
};
use bytes::{Bytes, BytesMut};
use fnv::FnvHashMap;
use std::net::Ipv4Addr;
use tokio::{
	io::{AsyncReadExt, AsyncWriteExt},
	net::TcpStream,
	time::{sleep, Duration},
};

/// Get parameters from the parameter space of a MION bridge.
///
/// These are the parameters you can access from the normal CLI tools:
///
/// - `mionps.exe`
/// - `mionparameterspace.exe`
///
/// It's unclear what many of these parameters are, but we know it contains at
/// least certain values like the SDK version, NAND Mode, etc.
///
/// ## Errors
///
/// - If we fail to connect, send, or receive data from the MION IP Address
///   on the [`MION_PARAMETER_PORT`].
/// - If we do not get a response within [`MION_PARAMETER_TIMEOUT_SECONDS`].
/// - If the MION responded with invalid data.
pub async fn get_parameters(
	mion_addr: Ipv4Addr,
	timeout: Option<Duration>,
) -> Result<DumpedMionParameters, CatBridgeError> {
	get_parameters_with_logging_hooks(
		mion_addr,
		timeout,
		noop_tcp_session_made,
		noop_connection_established,
		noop_write_finished,
		noop_read_finished,
	)
	.await
}

/// Get parameters from the parameter space of a MION bridge.
///
/// These are the parameters you can access from the normal CLI tools:
///
/// - `mionps.exe`
/// - `mionparameterspace.exe`
///
/// It's unclear what many of these parameters are, but we know it contains at
/// least certain values like the SDK version, NAND Mode, etc.
///
/// This is the function that allows you to specify EXTRA logging hooks (e.g.
/// those that aren't written to [`tracing`], for like when you need to manually
/// recreate a CLI with old hacky `println!`).
///
/// You probably want [`get_parameters`].
///
/// ## Errors
///
/// See [`get_parameters`].
pub async fn get_parameters_with_logging_hooks<
	TcpSessionHook,
	ConnectionEstablishedHook,
	WriteFinishedHook,
	ReadFinishedHook,
>(
	mion_addr: Ipv4Addr,
	timeout: Option<Duration>,
	tcp_session_logging_hook: TcpSessionHook,
	connection_established_logging_hook: ConnectionEstablishedHook,
	write_finished_hook: WriteFinishedHook,
	read_finished_hook: ReadFinishedHook,
) -> Result<DumpedMionParameters, CatBridgeError>
where
	TcpSessionHook: Fn(u128) + Clone + Send + 'static,
	ConnectionEstablishedHook: Fn(Ipv4Addr) + Clone + Send + 'static,
	WriteFinishedHook: Fn(usize) + Clone + Send + 'static,
	ReadFinishedHook: Fn(usize) + Clone + Send + 'static,
{
	let usable_timeout = timeout.unwrap_or(Duration::from_secs(MION_PARAMETER_TIMEOUT_SECONDS));
	// The logging hook expects a millisecond timeout.
	tcp_session_logging_hook(usable_timeout.as_millis());

	tokio::select! {
	  res = get_parameters_without_timeout(
			mion_addr,
			connection_established_logging_hook,
			write_finished_hook,
			read_finished_hook,
		) => { res.map(|(params, _stream)| params) }
	  () = sleep(usable_timeout) => {
		  Err(CatBridgeError::NetworkError(NetworkError::TimeoutError))
	  }
	}
}

/// Set one or more parameters for the parameter space of a MION bridge.
///
/// These are the parameters you can access from the normal CLI tools:
///
/// - `mionps.exe`
/// - `mionparameterspace.exe`
///
/// It's unclear what many of these parameters are, but we know it contains at
/// least certain values like the SDK version, NAND Mode, etc.
///
/// ## Errors
///
/// - If we fail to connect, send, or receive data from the MION IP Address
///   on the [`MION_PARAMETER_PORT`].
/// - If we do not get a response within [`MION_PARAMETER_TIMEOUT_SECONDS`].
/// - If the MION responded with invalid data.
/// - If the MION responds with a non successful status code.
pub async fn set_parameters<IterTy>(
	parameters_to_set: IterTy,
	mion_addr: Ipv4Addr,
	timeout: Option<Duration>,
) -> Result<SetMionParametersResponse, CatBridgeError>
where
	IterTy: Iterator<Item = (ParameterLocationSpecification, u8)>,
{
	set_parameters_with_logging_hooks(
		parameters_to_set,
		mion_addr,
		timeout,
		noop_tcp_session_made,
		noop_connection_established,
		noop_write_finished,
		noop_read_finished,
		noop_set_value_hook,
		noop_write_finished,
	)
	.await
	.map(|(resp, _changed_values)| resp)
}

/// Set one or more parameters for the parameter space of a MION bridge.
///
/// These are the parameters you can access from the normal CLI tools:
///
/// - `mionps.exe`
/// - `mionparameterspace.exe`
///
/// It's unclear what many of these parameters are, but we know it contains at
/// least certain values like the SDK version, NAND Mode, etc.
///
/// This function is like set parameters, but it also returns a map of:
/// `<location, old_value>` for each changed value, so you can print out
/// what the value was changed from/to.
///
/// ## Errors
///
/// See [`set_parameters`].
pub async fn set_parameters_and_get_changed_values<IterTy>(
	parameters_to_set: IterTy,
	mion_addr: Ipv4Addr,
	timeout: Option<Duration>,
) -> Result<(SetMionParametersResponse, FnvHashMap<usize, u8>), CatBridgeError>
where
	IterTy: Iterator<Item = (ParameterLocationSpecification, u8)>,
{
	set_parameters_with_logging_hooks(
		parameters_to_set,
		mion_addr,
		timeout,
		noop_tcp_session_made,
		noop_connection_established,
		noop_write_finished,
		noop_read_finished,
		noop_set_value_hook,
		noop_write_finished,
	)
	.await
}

/// Set one or more parameters for the parameter space of a MION bridge.
///
/// These are the parameters you can access from the normal CLI tools:
///
/// - `mionps.exe`
/// - `mionparameterspace.exe`
///
/// It's unclear what many of these parameters are, but we know it contains at
/// least certain values like the SDK version, NAND Mode, etc.
///
/// This is the function that allows you to specify EXTRA logging hooks (e.g.
/// those that aren't written to [`tracing`], for like when you need to manually
/// recreate a CLI with old hacky `println!`).
///
/// You probably want [`set_parameters`].
///
/// ## Errors
///
/// See [`set_parameters`].
#[allow(
	// Yes, clippy I KNOW THIS IS BAD. I HATE IT TOO.
	clippy::too_many_arguments,
)]
pub async fn set_parameters_with_logging_hooks<
	IterTy,
	TcpSessionHook,
	ConnectionEstablishedHook,
	WriteFinishedHook,
	ReadFinishedHook,
	SetNewValueHook,
	WriteSetFinishedHook,
>(
	parameters_to_set: IterTy,
	mion_addr: Ipv4Addr,
	timeout: Option<Duration>,
	tcp_session_logging_hook: TcpSessionHook,
	connection_established_logging_hook: ConnectionEstablishedHook,
	write_finished_hook: WriteFinishedHook,
	read_finished_hook: ReadFinishedHook,
	set_new_value_hook: SetNewValueHook,
	write_set_finished_hook: WriteSetFinishedHook,
) -> Result<(SetMionParametersResponse, FnvHashMap<usize, u8>), CatBridgeError>
where
	IterTy: Iterator<Item = (ParameterLocationSpecification, u8)>,
	TcpSessionHook: Fn(u128) + Clone + Send + 'static,
	ConnectionEstablishedHook: Fn(Ipv4Addr) + Clone + Send + 'static,
	WriteFinishedHook: Fn(usize) + Clone + Send + 'static,
	ReadFinishedHook: Fn(usize) + Clone + Send + 'static,
	SetNewValueHook: Fn(u8, u8, usize) + Clone + Send + 'static,
	WriteSetFinishedHook: Fn(usize) + Clone + Send + 'static,
{
	let usable_timeout = timeout.unwrap_or(Duration::from_secs(MION_PARAMETER_TIMEOUT_SECONDS));
	// The logging hook expects a millisecond timeout.
	tcp_session_logging_hook(usable_timeout.as_millis());

	let (got_parameters, stream) = tokio::select! {
	  res = get_parameters_without_timeout(
			mion_addr,
			connection_established_logging_hook,
			write_finished_hook,
			read_finished_hook,
		) => { res }
	  () = sleep(usable_timeout) => {
		  Err(CatBridgeError::NetworkError(NetworkError::TimeoutError))
	  }
	}?;

	let mut old_values_map = FnvHashMap::default();
	let mut new_parameters = BytesMut::with_capacity(512);
	new_parameters.extend_from_slice(got_parameters.get_raw_parameters());
	for (location_spec, new_value) in parameters_to_set {
		let location = match location_spec {
			ParameterLocationSpecification::Index(idx) => usize::from(idx),
			ParameterLocationSpecification::NameLike(name) => {
				index_from_parameter_name(&name).ok_or(APIError::MIONParameterNameNotKnown(name))?
			}
		};

		let orig_value = got_parameters.get_raw_parameters()[location];
		set_new_value_hook(orig_value, new_value, location);
		old_values_map.insert(location, orig_value);
		new_parameters[location] = new_value;
	}

	tokio::select! {
	  res = set_parameters_without_timeout(
			new_parameters.freeze(),
			stream,
			write_set_finished_hook,
		) => { res.map(|success| (success, old_values_map)) }
	  () = sleep(usable_timeout) => {
		  Err(CatBridgeError::NetworkError(NetworkError::TimeoutError))
	  }
	}
}

async fn get_parameters_without_timeout<
	ConnectionEstablishedHook,
	WriteFinishedHook,
	ReadFinishedHook,
>(
	mion_addr: Ipv4Addr,
	connection_established_hook: ConnectionEstablishedHook,
	write_finished_hook: WriteFinishedHook,
	read_finished_hook: ReadFinishedHook,
) -> Result<(DumpedMionParameters, TcpStream), CatBridgeError>
where
	ConnectionEstablishedHook: Fn(Ipv4Addr) + Clone + Send + 'static,
	WriteFinishedHook: Fn(usize) + Clone + Send + 'static,
	ReadFinishedHook: Fn(usize) + Clone + Send + 'static,
{
	let mut stream = TcpStream::connect((mion_addr, MION_PARAMETER_PORT))
		.await
		.map_err(NetworkError::IOError)?;
	connection_established_hook(mion_addr);
	stream.writable().await.map_err(NetworkError::IOError)?;
	stream
		.write(&Bytes::from(MionDumpParameters::new()))
		.await
		.map_err(NetworkError::IOError)?;

	let expected_bytes_to_read = 520;
	write_finished_hook(expected_bytes_to_read);

	let mut resp_buff = BytesMut::with_capacity(expected_bytes_to_read);
	let read_bytes = stream
		.read_buf(&mut resp_buff)
		.await
		.map_err(NetworkError::IOError)?;
	read_finished_hook(read_bytes);
	if read_bytes != expected_bytes_to_read {
		return Err(CatBridgeError::NetworkError(NetworkError::ParseError(
			NetworkParseError::NotEnoughData(
				"DumpedMionParameters",
				expected_bytes_to_read,
				read_bytes,
				resp_buff.freeze(),
			),
		)));
	}
	let parameters = DumpedMionParameters::try_from(resp_buff.freeze())?;

	Ok((parameters, stream))
}

async fn set_parameters_without_timeout<WriteFinishedHook>(
	new_parameters: Bytes,
	mut stream: TcpStream,
	write_finished_hook: WriteFinishedHook,
) -> Result<SetMionParametersResponse, CatBridgeError>
where
	WriteFinishedHook: Fn(usize) + Clone + Send + 'static,
{
	stream.writable().await.map_err(NetworkError::IOError)?;
	stream
		.write(&Bytes::from(SetMionParameters::new(new_parameters)?))
		.await
		.map_err(NetworkError::IOError)?;

	let expected_bytes_to_read = 12;
	write_finished_hook(expected_bytes_to_read);

	let mut resp_buff = BytesMut::with_capacity(expected_bytes_to_read);
	let read_bytes = stream
		.read_buf(&mut resp_buff)
		.await
		.map_err(NetworkError::IOError)?;
	if read_bytes != expected_bytes_to_read {
		return Err(CatBridgeError::NetworkError(NetworkError::ParseError(
			NetworkParseError::NotEnoughData(
				"SetMionParametersResponse",
				expected_bytes_to_read,
				read_bytes,
				resp_buff.freeze(),
			),
		)));
	}
	let response = SetMionParametersResponse::try_from(resp_buff.freeze())?;

	Ok(response)
}

fn noop_tcp_session_made(_timeout: u128) {}

fn noop_connection_established(_ip: Ipv4Addr) {}

fn noop_write_finished(_expected_read: usize) {}

fn noop_read_finished(_read_size: usize) {}

fn noop_set_value_hook(_old_value: u8, _new_value: u8, _location: usize) {}
