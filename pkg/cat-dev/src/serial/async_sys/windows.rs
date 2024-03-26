//! Wrapper around a serial port that can be talked to asynchrnously for
//! Windows.
//!
//! This wraps a serial port from the underlying library in a
//! [`NamedPipeClient`] which will allow for us to interact with the port
//! asynchronously safely.

use crate::serial::underlying::SyncSerialPort;
use std::{
	io::{Error as IoError, ErrorKind as IoErrorKind, IoSlice, IoSliceMut, Result as IoResult},
	mem::{forget, ManuallyDrop},
	os::windows::io::{AsRawHandle, FromRawHandle},
	pin::Pin,
	task::{Context, Poll},
	time::Duration,
};
use tokio::{
	io::{AsyncRead, AsyncWrite, ReadBuf},
	net::windows::named_pipe::NamedPipeClient,
};

/// Thin wrapper around a serial port that can be interacted with
/// asynchronously.
pub struct RawAsyncSerialPort {
	io: NamedPipeClient,
}

impl RawAsyncSerialPort {
	/// Attempt to create a new wrapper around an existing serial port.
	///
	/// ## Errors
	///
	/// See:
	///
	///   - [`SyncSerialPort::set_read_timeout`]
	///   - [`SyncSerialPort::set_write_timeout`]
	///   - [`NamedPipeClient::from_raw_handle`]
	///
	/// for error descriptons on why this can fail.
	pub fn new(mut inner: SyncSerialPort) -> IoResult<Self> {
		// We don't want timeouts on the operations themselves.
		// The user can use `tokio::time::timeout()` if they want.
		inner.set_read_timeout(Duration::from_millis((u32::MAX - 1).into()))?;
		inner.set_write_timeout(Duration::from_millis((u32::MAX - 1).into()))?;

		// First try to convert the inner serial port to a `NamedPipeClient`.
		// Only when that succeeded relinquish ownership of the file handle by forggeting `inner`.
		let io = unsafe { NamedPipeClient::from_raw_handle(inner.as_raw_handle())? };
		forget(inner);

		Ok(Self { io })
	}

	/// Attempt to clone this object.
	///
	/// ## Errors
	///
	/// See: [`SyncSerialPort::try_clone`] for error descriptions on why this can fail.
	pub fn try_clone(&self) -> IoResult<Self> {
		Self::new(self.with_raw(SyncSerialPort::try_clone)?)
	}

	/// Access the underlying serial port object directly.
	pub fn with_raw<F, R>(&self, function: F) -> R
	where
		F: FnOnce(&SyncSerialPort) -> R,
	{
		let serial_port =
			ManuallyDrop::new(unsafe { SyncSerialPort::from_raw_handle(self.io.as_raw_handle()) });
		function(&serial_port)
	}

	/// Attempt to read from the serial port.
	///
	/// ## Errors
	///
	/// if [`NamedPipeClient::try_read`] returns an error when attempting to
	/// read from the serial port.
	pub async fn read(&self, buf: &mut [u8]) -> IoResult<usize> {
		loop {
			self.io.readable().await?;
			match self.io.try_read(buf) {
				Ok(read) => return Ok(read),
				Err(cause) => {
					if cause.kind() == IoErrorKind::WouldBlock {
						continue;
					}

					return Err(cause);
				}
			}
		}
	}

	/// If this implementation supports vectored reads.
	#[must_use]
	pub const fn can_read_vectored() -> bool {
		false
	}

	/// Attempt to read from multiple ranges at once, aka perform a "vectored"
	/// read.
	///
	/// ## Errors
	///
	/// Always, unfortunately named pipes (what we're using to interact
	/// asynchronously with the serial port), does not support vectored
	/// operations.
	#[allow(
		// Keep us equal with nix.
		unused_variables,
		// Need to match API with linux which does need an await.
		clippy::unused_async,
	)]
	pub async fn read_vectored(&self, bufs: &mut [IoSliceMut<'_>]) -> IoResult<usize> {
		Err(IoError::from_raw_os_error(
			windows::Win32::Networking::WinSock::WSAENOTSOCK.0,
		))
	}

	/// Attempt to write to the underlying serial port.
	///
	/// ## Errors
	///
	/// if [`NamedPipeClient::try_write`] returns an error code.
	pub async fn write(&self, buf: &[u8]) -> IoResult<usize> {
		loop {
			self.io.writable().await?;
			match self.io.try_write(buf) {
				Ok(n) => return Ok(n),
				Err(cause) => {
					if cause.kind() == IoErrorKind::WouldBlock {
						continue;
					}

					return Err(cause);
				}
			}
		}
	}

	/// If this implementation supports vectored writes.
	#[must_use]
	pub const fn can_write_vectored() -> bool {
		false
	}

	/// Attempt to write to multiple ranges at once, aka perform a "vectored"
	/// write.
	///
	/// ## Errors
	///
	/// Always, unfortunately named pipes (what we're using to interact
	/// asynchronously with the serial port), does not support vectored
	/// operations.
	#[allow(
		// This is needed to keep us in sync with unix.
		unused_variables,
		// Need to match API with linux which does need an await.
		clippy::unused_async,
	)]
	pub async fn write_vectored(&self, bufs: &[IoSlice<'_>]) -> IoResult<usize> {
		Err(IoError::from_raw_os_error(
			windows::Win32::Networking::WinSock::WSAENOTSOCK.0,
		))
	}

	/// Attempt to perform a polled read on the underlying serial connection.
	pub fn poll_read(&mut self, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<IoResult<()>> {
		AsyncRead::poll_read(Pin::new(&mut self.io), cx, buf)
	}

	/// Attempt to perform a polled write on the underlying serial connection.
	pub fn poll_write(&mut self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<IoResult<usize>> {
		AsyncWrite::poll_write(Pin::new(&mut self.io), cx, buf)
	}

	/// Attempt to perform a polled vectored write on the underlying serial
	/// connection.
	#[allow(
		// Kept us equal with nix in terms of api..
		unused_variables,
		// Not yet implemented
		clippy::unused_self,
	)]
	pub fn poll_write_vectored(
		&mut self,
		ctx: &mut Context<'_>,
		bufs: &[IoSlice<'_>],
	) -> Poll<IoResult<usize>> {
		Poll::Ready(Err(IoError::from_raw_os_error(
			windows::Win32::Networking::WinSock::WSAENOTSOCK.0,
		)))
	}

	/// Perform a polled shutdown.
	///
	/// *note: this will always return an error as a serial port cannot be
	/// shutdown.*
	#[allow(
		// Cannot be implemented but needs to match unix.
		clippy::unused_self,
	)]
	pub fn poll_shutdown(&mut self, _cx: &mut Context<'_>) -> Poll<IoResult<()>> {
		Poll::Ready(Err(IoError::from_raw_os_error(
			windows::Win32::Networking::WinSock::WSAENOTSOCK.0,
		)))
	}
}
