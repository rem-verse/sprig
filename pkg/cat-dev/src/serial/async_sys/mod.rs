//! The OS specifical asynchronous serial port implementation.
//!
//! This module provides [`RawAsyncSerialPort`], a very thin wrapper around
//! the serial port provided by the underlying implementation using tokio's
//! an asynchronous file descriptor on unix, or on windows wrapping in a
//! named pipe client so we interact with the port asynchronously safely.

#[cfg(any(
	target_os = "linux",
	target_os = "freebsd",
	target_os = "openbsd",
	target_os = "netbsd",
	target_os = "macos"
))]
mod unix;
#[cfg(target_os = "windows")]
mod windows;

use tokio::io::{AsyncRead, AsyncWrite};
#[cfg(any(
	target_os = "linux",
	target_os = "freebsd",
	target_os = "openbsd",
	target_os = "netbsd",
	target_os = "macos"
))]
use unix::RawAsyncSerialPort;
#[cfg(target_os = "windows")]
use windows::RawAsyncSerialPort;

use crate::serial::SyncSerialPort;
use std::{
	io::{IoSlice, IoSliceMut, Result as IoResult},
	path::{Path, PathBuf},
	pin::Pin,
	task::{Context, Poll},
};
use tokio::io::ReadBuf;

/// An asynchronous serial port.
pub struct AsyncSerialPort {
	inner: RawAsyncSerialPort,
}

impl AsyncSerialPort {
	/// Get a list of available serial ports.
	///
	/// ## Errors
	///
	/// If your platform is unsupported, or an OS error occurs.
	pub fn available_ports() -> IoResult<Vec<PathBuf>> {
		SyncSerialPort::available_ports()
	}

	/// Open and configure a serial port by path or name.
	///
	/// On Unix systems, the `name` parameter must be a path to a TTY device. On
	/// Windows, it must be the name of a COM device, such as COM1, COM2, etc.
	///
	/// The library automatically uses the win32 device namespace on Windows, so
	/// COM ports above COM9 are supported out of the box.
	///
	/// ## Errors
	///
	/// If we cannot open, or configure the serial device at path.
	pub fn new(path: impl AsRef<Path>) -> IoResult<Self> {
		Ok(Self {
			inner: RawAsyncSerialPort::new(SyncSerialPort::new(path)?)?,
		})
	}

	/// Try to clone the serial port handle.
	///
	/// The cloned object refers to the same serial port.
	///
	/// Mixing reads and writes on different handles to the same serial port
	/// from different threads may lead to unexpect results. The data may end
	/// up interleaved in unpredictable ways.
	///
	/// ## Errors
	///
	/// If we cannot clone the underlying file descriptor.
	pub fn try_clone(&self) -> IoResult<Self> {
		let inner = self.inner.try_clone()?;
		Ok(Self { inner })
	}

	/// Read bytes from the serial port.
	///
	/// This is identical to
	/// [`AsyncReadExt::read()`][tokio::io::AsyncReadExt::read], except that
	/// this function takes a const reference `&self`. This allows you to use
	/// the serial port concurrently from multiple tasks.
	///
	/// Note that there are no guarantees about which task receives what data
	/// when multiple tasks are reading from the serial port. You should normally
	/// limit yourself to a single reading task and a single writing task.
	///
	/// ## Errors
	///
	/// If the underlying OS, or device throws an error.
	pub async fn read(&self, buff: &mut [u8]) -> IoResult<usize> {
		self.inner.read(buff).await
	}

	/// If this implementation supports vectored reads.
	#[must_use]
	pub const fn can_read_vectored() -> bool {
		RawAsyncSerialPort::can_read_vectored()
	}

	/// Read bytes from the serial port into a slice of buffers.
	///
	/// Note that there are no guarantees about which task receives what data
	/// when multiple tasks are reading from the serial port. You should
	/// normally limit yourself to a single reading task and a single writing
	/// task.
	///
	/// ## Errors
	///
	/// If the underlying OS, or device throws an error.
	pub async fn read_vectored(&self, bufs: &mut [IoSliceMut<'_>]) -> IoResult<usize> {
		self.inner.read_vectored(bufs).await
	}

	/// Write bytes to the serial port.
	///
	/// This is identical to
	/// [`AsyncWriteExt::write()`][tokio::io::AsyncWriteExt::write], except that
	/// this function takes a const reference `&self`. This allows you to use the
	/// serial port concurrently from multiple tasks.
	///
	/// Note that data written to the same serial port from multiple tasks may
	/// end up interleaved at the receiving side. You should normally limit
	/// yourself to a single reading task and a single writing task.
	///
	/// ## Errors
	///
	/// If the underlying OS, or device throws an error.
	pub async fn write(&self, buff: &[u8]) -> IoResult<usize> {
		self.inner.write(buff).await
	}

	/// Write all bytes to the serial port.
	///
	/// This will continue to call [`Self::write()`] until the entire buffer
	/// has been written, or an I/O error occurs.
	///
	/// This is identical to
	/// [`AsyncWriteExt::write_all()`][tokio::io::AsyncWriteExt::write_all],
	/// except that this function takes a const reference `&self`. This allows
	/// you to use the serial port concurrently from multiple tasks.
	///
	/// Note that data written to the same serial port from multiple tasks may
	/// end up interleaved at the receiving side. You should normally limit
	/// yourself to a single reading task and a single writing task.
	///
	/// ## Errors
	///
	/// If the underlying OS, or device throws an error.
	pub async fn write_all(&self, buff: &[u8]) -> IoResult<()> {
		let mut written = 0;
		while written < buff.len() {
			written += self.write(&buff[written..]).await?;
		}
		Ok(())
	}

	/// If this implementation supports vectored writes.
	#[must_use]
	pub const fn can_write_vectored() -> bool {
		RawAsyncSerialPort::can_write_vectored()
	}

	/// Write bytes to the serial port from a slice of buffers.
	///
	/// This is identical to
	/// [`AsyncWriteExt::write_vectored()`][tokio::io::AsyncWriteExt::write_vectored],
	/// except that this function takes a const reference `&self`. This allows
	/// you to use the serial port concurrently from multiple tasks.
	///
	/// Note that data written to the same serial port from multiple tasks may
	/// end up interleaved at the receiving side. You should normally limit
	/// yourself to a single reading task and a single writing task.
	///
	/// ## Errors
	///
	/// If the underlying OS, or device throws an error.
	pub async fn write_vectored(&self, bufs: &[IoSlice<'_>]) -> IoResult<usize> {
		self.inner.write_vectored(bufs).await
	}

	/// Discard the kernel input and output buffers for the serial port.
	///
	/// When you write to a serial port, the data may be put in a buffer by the
	/// OS to be transmitted by the actual device later. Similarly, data received
	/// on the device can be put in a buffer by the OS untill you read it. This
	/// function clears both buffers: any untransmitted data and received but
	/// unread data is discarded by the OS.
	///
	/// ## Errors
	///
	/// If the underlying OS, or device throws an error.
	pub fn discard_buffers(&self) -> IoResult<()> {
		self.inner.with_raw(SyncSerialPort::discard_buffers)
	}

	/// Discard the kernel input buffers for the serial port.
	///
	/// Data received on the device can be put in a buffer by the OS untill
	/// you read it. This function clears that buffer: received but unread
	/// data is discarded by the OS.
	///
	/// This is particularly useful when communicating with a device that only
	/// responds to commands that you send to it. If you discard the input
	/// buffer before sending the command, you discard any noise that may have
	/// been received after the last command.
	///
	/// ## Errors
	///
	/// If the underlying OS, or device throws an error.
	pub fn discard_input_buffer(&self) -> IoResult<()> {
		self.inner.with_raw(SyncSerialPort::discard_input_buffer)
	}

	/// Discard the kernel output buffers for the serial port.
	///
	/// When you write to a serial port, the data is generally put in a buffer
	/// by the OS to be transmitted by the actual device later. This function
	/// clears that buffer: any untransmitted data is discarded by the OS.
	///
	/// ## Errors
	///
	/// If the underlying OS, or device throws an error.
	pub fn discard_output_buffer(&self) -> IoResult<()> {
		self.inner.with_raw(SyncSerialPort::discard_output_buffer)
	}

	/// Set the state of the Ready To Send line.
	///
	/// If hardware flow control is enabled on the serial port, it is platform
	/// specific what will happen. The function may fail with an error or it
	/// may silently be ignored. It may even succeed and interfere with the
	/// flow control.
	///
	/// ## Errors
	///
	/// If the underlying OS, or device throws an error.
	pub fn set_rts(&self, state: bool) -> IoResult<()> {
		self.inner.with_raw(|raw| raw.set_rts(state))
	}

	/// Read the state of the Clear To Send line.
	///
	/// If hardware flow control is enabled on the serial port, it is platform
	/// specific what will happen. The function may fail with an error, it
	/// may return a bogus value, or it may return the actual state of the CTS
	/// line.
	///
	/// ## Errors
	///
	/// If the underlying OS, or device throws an error.
	pub fn read_cts(&self) -> IoResult<bool> {
		self.inner.with_raw(SyncSerialPort::read_cts)
	}

	/// Set the state of the Data Terminal Ready line.
	///
	/// If hardware flow control is enabled on the serial port, it is platform
	/// specific what will happen. The function may fail with an error or it
	/// may silently be ignored.
	///
	/// ## Errors
	///
	/// If the underlying OS, or device throws an error.
	pub fn set_dtr(&self, state: bool) -> IoResult<()> {
		self.inner.with_raw(|raw| raw.set_dtr(state))
	}

	/// Read the state of the Data Set Ready line.
	///
	/// If hardware flow control is enabled on the serial port, it is platform
	/// specific what will happen. The function may fail with an error, it may
	/// return a bogus value, or it may return the actual state of the DSR line.
	///
	/// ## Errors
	///
	/// If the underlying OS, or device throws an error.
	pub fn read_dsr(&self) -> IoResult<bool> {
		self.inner.with_raw(SyncSerialPort::read_dsr)
	}

	/// Read the state of the Ring Indicator line.
	///
	/// This line is also sometimes also called the RNG or RING line.
	///
	/// ## Errors
	///
	/// If the underlying OS, or device throws an error.
	pub fn read_ri(&self) -> IoResult<bool> {
		self.inner.with_raw(SyncSerialPort::read_ri)
	}

	/// Read the state of the Carrier Detect (CD) line.
	///
	/// This line is also called the Data Carrier Detect (DCD) line
	/// or the Receive Line Signal Detect (RLSD) line.
	///
	/// ## Errors
	///
	/// If the underlying OS, or device throws an error.
	pub fn read_cd(&self) -> IoResult<bool> {
		self.inner.with_raw(SyncSerialPort::read_cd)
	}
}

impl AsyncRead for AsyncSerialPort {
	fn poll_read(
		self: Pin<&mut Self>,
		ctx: &mut Context<'_>,
		buff: &mut ReadBuf<'_>,
	) -> Poll<IoResult<()>> {
		self.get_mut().inner.poll_read(ctx, buff)
	}
}

impl AsyncWrite for AsyncSerialPort {
	fn poll_write(
		self: Pin<&mut Self>,
		ctx: &mut Context<'_>,
		buff: &[u8],
	) -> Poll<IoResult<usize>> {
		self.get_mut().inner.poll_write(ctx, buff)
	}

	fn poll_write_vectored(
		self: Pin<&mut Self>,
		ctx: &mut Context<'_>,
		bufs: &[IoSlice<'_>],
	) -> Poll<IoResult<usize>> {
		self.get_mut().inner.poll_write_vectored(ctx, bufs)
	}

	fn poll_flush(self: Pin<&mut Self>, _ctx: &mut Context<'_>) -> Poll<IoResult<()>> {
		// We can't do `tcdrain()` asynchronously :(
		Poll::Ready(Ok(()))
	}

	fn poll_shutdown(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<IoResult<()>> {
		self.get_mut().inner.poll_shutdown(ctx)
	}
}
