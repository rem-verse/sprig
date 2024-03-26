//! Wrapper around a serial port that can be talked to asynchrnously for unix.
//!
//! This wraps a serial port from the underlying library in [`AsyncFd`] which
//! will allow for us to interact with the port asynchronously safely.

use crate::serial::underlying::SyncSerialPort;
use std::{
	io::{Error as IoError, IoSlice, IoSliceMut, Result as IoResult},
	os::fd::AsRawFd,
	task::{ready, Context, Poll},
};
use tokio::io::{unix::AsyncFd, Interest, ReadBuf};

/// Thin wrapper around a serial port that can be interacted with
/// asynchronously.
pub struct RawAsyncSerialPort {
	io: AsyncFd<SyncSerialPort>,
}

impl RawAsyncSerialPort {
	/// Attempt to create a new wrapper around an existing serial port.
	///
	/// ## Errors
	///
	/// See: [`AsyncFd::new`] for error descriptons on why this can fail.
	pub fn new(inner: SyncSerialPort) -> IoResult<Self> {
		Ok(Self {
			io: AsyncFd::new(inner)?,
		})
	}

	/// Attempt to clone this object.
	///
	/// ## Errors
	///
	/// See: [`SyncSerialPort::try_clone`] for error descriptions on why this can fail.
	pub fn try_clone(&self) -> IoResult<Self> {
		Self::new(self.io.get_ref().try_clone()?)
	}

	/// Access the underlying serial port object directly.
	pub fn with_raw<F, R>(&self, function: F) -> R
	where
		F: FnOnce(&SyncSerialPort) -> R,
	{
		function(self.io.get_ref())
	}

	/// Attempt to read from the serial port.
	///
	/// ## Errors
	///
	/// if [`libc::read`] returns an error when attempting to read from the
	/// serial port.
	pub async fn read(&self, buf: &mut [u8]) -> IoResult<usize> {
		self.io
			.async_io(Interest::READABLE, |inner| unsafe {
				error_code_to_io_result(libc::read(
					inner.as_raw_fd(),
					buf.as_mut_ptr().cast(),
					buf.len(),
				))
			})
			.await
	}

	/// If this implementation supports vectored reads.
	#[must_use]
	pub const fn can_read_vectored() -> bool {
		true
	}

	/// Attempt to read from multiple ranges at once, aka perform a "vectored"
	/// read.
	///
	/// ## Errors
	///
	/// if [`libc::readv`] returns an error when attempting to read from the
	/// serial port.
	pub async fn read_vectored(&self, bufs: &mut [IoSliceMut<'_>]) -> IoResult<usize> {
		self.io
			.async_io(Interest::READABLE, |inner| unsafe {
				let buf_count = i32::try_from(bufs.len()).unwrap_or(i32::MAX);
				error_code_to_io_result(libc::readv(
					inner.as_raw_fd(),
					bufs.as_mut_ptr().cast(),
					buf_count,
				))
			})
			.await
	}

	/// Attempt to write to the underlying serial port.
	///
	/// ## Errors
	///
	/// if [`libc::write`] returns an error code.
	pub async fn write(&self, buf: &[u8]) -> IoResult<usize> {
		self.io
			.async_io(Interest::WRITABLE, |inner| unsafe {
				error_code_to_io_result(libc::write(
					inner.as_raw_fd(),
					buf.as_ptr().cast(),
					buf.len(),
				))
			})
			.await
	}

	/// If this implementation supports vectored writes.
	#[must_use]
	pub const fn can_write_vectored() -> bool {
		true
	}

	/// Atttempt to perform a vectored write to the serial port (writing multiple
	/// ranges at the same time).
	///
	/// ## Errors
	///
	/// If [`libc::writev`] returns an error when attempting to write to the
	/// serial port.
	pub async fn write_vectored(&self, bufs: &[IoSlice<'_>]) -> IoResult<usize> {
		self.io
			.async_io(Interest::WRITABLE, |inner| unsafe {
				let buf_count = i32::try_from(bufs.len()).unwrap_or(i32::MAX);
				error_code_to_io_result(libc::writev(
					inner.as_raw_fd(),
					bufs.as_ptr().cast(),
					buf_count,
				))
			})
			.await
	}

	/// Attempt to perform a polled read on the underlying serial connection.
	pub fn poll_read(&mut self, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<IoResult<()>> {
		loop {
			let mut guard = ready!(self.io.poll_read_ready(cx)?);
			let result = guard.try_io(|inner| unsafe {
				let unfilled = buf.unfilled_mut();
				error_code_to_io_result(libc::read(
					inner.as_raw_fd(),
					unfilled.as_mut_ptr().cast(),
					unfilled.len(),
				))
			});
			match result {
				Ok(result) => {
					let read = result?;
					unsafe { buf.assume_init(read) };
					buf.advance(read);
					return Poll::Ready(Ok(()));
				}
				Err(_would_block) => continue,
			}
		}
	}

	/// Attempt to perform a polled write on the underlying serial connection.
	pub fn poll_write(&mut self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<IoResult<usize>> {
		loop {
			let mut guard = ready!(self.io.poll_write_ready(cx)?);
			let result = guard.try_io(|inner| {
				error_code_to_io_result(unsafe {
					libc::write(inner.as_raw_fd(), buf.as_ptr().cast(), buf.len())
				})
			});
			match result {
				Ok(result) => return Poll::Ready(result),
				Err(_would_block) => continue,
			}
		}
	}

	/// Attempt to perform a polled vectored write on the underlying serial
	/// connection.
	pub fn poll_write_vectored(
		&mut self,
		cx: &mut Context<'_>,
		bufs: &[IoSlice<'_>],
	) -> Poll<IoResult<usize>> {
		loop {
			let mut guard = ready!(self.io.poll_write_ready(cx)?);
			let result = guard.try_io(|inner| {
				let buf_count = i32::try_from(bufs.len()).unwrap_or(i32::MAX);
				error_code_to_io_result(unsafe {
					libc::writev(inner.as_raw_fd(), bufs.as_ptr().cast(), buf_count)
				})
			});
			match result {
				Ok(result) => return Poll::Ready(result),
				Err(_would_block) => continue,
			}
		}
	}

	/// Perform a polled shutdown.
	///
	/// *note: this will always return an error as a serial port cannot be
	/// shutdown.*
	#[allow(
		// Just unimplemented for now.
		clippy::unused_self,
	)]
	pub fn poll_shutdown(&mut self, _cx: &mut Context<'_>) -> Poll<IoResult<()>> {
		Poll::Ready(Err(IoError::from_raw_os_error(libc::ENOTSOCK)))
	}
}

/// Convert a potential error code into a result object.
#[allow(
	// We manually check this sign to make sure it is not an issue.
	clippy::cast_sign_loss,
)]
fn error_code_to_io_result(value: isize) -> IoResult<usize> {
	if value < 0 {
		Err(IoError::last_os_error())
	} else {
		Ok(value as usize)
	}
}
