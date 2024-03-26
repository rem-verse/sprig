mod sys;

use crate::serial::underlying::sys::RawSyncSerialPort;
use std::{
	io::{
		Error as IoError, ErrorKind as IoErrorKind, IoSlice, IoSliceMut, Read, Result as IoResult,
		Write,
	},
	path::{Path, PathBuf},
	time::Duration,
};

#[cfg(unix)]
use crate::serial::underlying::sys::DEFAULT_TIMEOUT_MS;

/// A serial port that you can interact with synchronously.
#[derive(Debug)]
pub struct SyncSerialPort {
	inner: RawSyncSerialPort,
}

impl SyncSerialPort {
	/// Get a list of available serial ports.
	///
	/// ## Errors
	///
	/// - If the platform is not supported.
	/// - If we get an error from the OS listing ports.
	pub fn available_ports() -> IoResult<Vec<PathBuf>> {
		RawSyncSerialPort::enumerate()
	}

	/// Open and configure a serial port by path or name.
	///
	/// On Unix systems, the `name` parameter must be a path to a TTY device.
	/// On Windows, it must be the name of a COM device, such as COM1, COM2, etc.
	///
	/// The library automatically uses the win32 device namespace on Windows,
	/// so COM ports above COM9 are supported out of the box.
	///
	/// ## Errors
	///
	/// If we cannot open the
	pub fn new(name: impl AsRef<Path>) -> IoResult<Self> {
		Ok(Self {
			inner: RawSyncSerialPort::new(name)?,
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
	/// If we cannot clone the underlying file descriptor/handle.
	pub fn try_clone(&self) -> IoResult<Self> {
		Ok(Self {
			inner: self.inner.try_clone()?,
		})
	}

	/// Read bytes from the serial port.
	///
	/// This is identical to [`std::io::Read::read()`], except that this function
	/// takes a const reference `&self`. This allows you to use the serial port
	/// concurrently from multiple threads.
	///
	/// Note that there are no guarantees on which thread receives what data
	/// when multiple threads are reading from the serial port. You should
	/// normally limit yourself to a single reading thread and a single
	/// writing thread.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn read(&self, buff: &mut [u8]) -> IoResult<usize> {
		self.inner.read(buff)
	}

	/// Read the exact number of bytes required to fill the buffer from the
	/// serial port.
	///
	/// This will repeatedly call `read()` until the entire buffer is filled.
	/// Errors of the type [`std::io::ErrorKind::Interrupted`] are silently
	/// ignored. Any other errors (including timeouts) will be returned
	/// immediately.
	///
	/// If this function returns an error, it may already have read some data
	/// from the serial port into the provided buffer.
	///
	/// This function is identical to [`std::io::Read::read_exact()`], except
	/// that this function takes a const reference `&self`. This allows you to
	/// use the serial port concurrently from multiple threads.
	///
	/// Note that there are no guarantees on which thread receives what data when
	/// multiple threads are reading from the serial port. You should normally
	/// limit yourself to a single reading thread and a single writing thread.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn read_exact(&self, buff: &mut [u8]) -> IoResult<()> {
		let mut working_buff = buff;

		while !working_buff.is_empty() {
			match self.read(working_buff) {
				Ok(0) => {
					return Err(IoError::new(
						IoErrorKind::UnexpectedEof,
						"Failed to fill whole buffer",
					))
				}
				Ok(read) => working_buff = &mut working_buff[read..],
				Err(cause) => {
					if cause.kind() == IoErrorKind::Interrupted {
						continue;
					}

					return Err(cause);
				}
			}
		}

		Ok(())
	}

	/// If this implementation supports vectored reads.
	#[must_use]
	pub const fn can_read_vectored() -> bool {
		RawSyncSerialPort::can_read_vectored()
	}

	/// Read bytes from the serial port into a slice of buffers.
	///
	/// This is identical to [`std::io::Read::read_vectored()`], except that this
	/// function takes a const reference `&self`. This allows you to use the
	/// serial port concurrently from multiple threads.
	///
	/// Note that there are no guarantees on which thread receives what data when
	/// multiple threads are reading from the serial port. You should normally
	/// limit yourself to a single reading thread and a single writing thread.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn read_vectored(&self, buff: &mut [IoSliceMut<'_>]) -> IoResult<usize> {
		self.inner.read_vectored(buff)
	}

	/// Write bytes to the serial port.
	///
	/// This is identical to [`std::io::Write::write()`], except that this
	/// function takes a const reference `&self`. This allows you to use the
	/// serial port concurrently from multiple threads.
	///
	/// Note that data written to the same serial port from multiple threads may
	/// end up interleaved at the receiving side. You should normally limit
	/// yourself to a single reading thread and a single writing thread.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn write(&self, buff: &[u8]) -> IoResult<usize> {
		self.inner.write(buff)
	}

	/// Write all bytes to the serial port.
	///
	/// This will continue to call [`Self::write()`] until the entire buffer has
	/// been written, or an I/O error occurs.
	///
	/// This is identical to [`std::io::Write::write_all()`], except that this
	/// function takes a const reference `&self`. This allows you to use the
	/// serial port concurrently from multiple threads.
	///
	/// Note that data written to the same serial port from multiple threads may
	/// end up interleaved at the receiving side. You should normally limit
	/// yourself to a single reading thread and a single writing thread.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn write_all(&self, buff: &[u8]) -> IoResult<()> {
		let mut working_buff = buff;

		while !working_buff.is_empty() {
			match self.write(working_buff) {
				Ok(0) => {
					return Err(IoError::new(
						IoErrorKind::WriteZero,
						"failed to write whole buffer",
					))
				}
				Ok(n) => working_buff = &working_buff[n..],
				Err(cause) => {
					if cause.kind() == IoErrorKind::Interrupted {
						continue;
					}

					return Err(cause);
				}
			}
		}

		Ok(())
	}

	/// If this implementation supports vectored writes.
	#[must_use]
	pub const fn can_write_vectored() -> bool {
		RawSyncSerialPort::can_write_vectored()
	}

	/// Write bytes to the serial port from a slice of buffers.
	///
	/// This is identical to [`std::io::Write::write_vectored()`], except that
	/// this function takes a const reference `&self`. This allows you to use
	/// the serial port concurrently from multiple threads.
	///
	/// Note that data written to the same serial port from multiple threads may
	/// end up interleaved at the receiving side. You should normally limit
	/// yourself to a single reading thread and a single writing thread.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn write_vectored(&self, buff: &[IoSlice<'_>]) -> IoResult<usize> {
		self.inner.write_vectored(buff)
	}

	/// Flush all data queued to be written.
	///
	/// This will block until the OS buffer has been fully transmitted.
	///
	/// This is identical to [`std::io::Write::flush()`], except that this
	/// function takes a const reference `&self`.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn flush(&self) -> IoResult<()> {
		self.inner.flush_output()
	}

	/// Get the read timeout for the serial port.
	///
	/// The timeout gotten by this function is an upper bound on individual calls
	/// to [`std::io::Read::read()`]. Other platform specific time-outs may
	/// trigger before this timeout does.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn get_read_timeout(&self) -> IoResult<Duration> {
		self.inner.get_read_timeout()
	}

	/// Get the write timeout for the serial port.
	///
	/// The timeout gotten by this function is an upper bound on individual calls
	/// to [`std::io::Write::write()`]. Other platform specific time-outs may
	/// trigger before this timeout does.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn get_write_timeout(&self) -> IoResult<Duration> {
		self.inner.get_write_timeout()
	}

	/// Set the read timeout for the serial port.
	///
	/// The timeout set by this function is an upper bound on individual calls
	/// to [`std::io::Read::read()`]. Other platform specific time-outs may
	/// trigger before this timeout does.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn set_read_timeout(&mut self, new_timeout: Duration) -> IoResult<()> {
		self.inner.set_read_timeout(new_timeout)
	}

	/// Set the read timeout for the serial port.
	///
	/// The timeout set by this function is an upper bound on individual calls
	/// to [`std::io::Write::write()`]. Other platform specific time-outs may
	/// trigger before this timeout does.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn set_write_timeout(&mut self, new_timeout: Duration) -> IoResult<()> {
		self.inner.set_write_timeout(new_timeout)
	}

	/// Discard the kernel input and output buffers for the serial port.
	///
	/// When you write to a serial port, the data may be put in a buffer
	/// by the OS to be transmitted by the actual device later. Similarly, data
	/// received on the device can be put in a buffer by the OS untill you read
	/// it. This function clears both buffers: any untransmitted data and
	/// received but unread data is discarded by the OS.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn discard_buffers(&self) -> IoResult<()> {
		self.inner.discard_buffers(true, true)
	}

	/// Discard the kernel input buffers for the serial port.
	///
	/// Data received on the device can be put in a buffer by the OS untill you
	/// read it. This function clears that buffer: received but unread data
	/// is discarded by the OS.
	///
	/// This is particularly useful when communicating with a device that only
	/// responds to commands that you send to it. If you discard the input
	/// buffer before sending the command, you discard any noise that may
	/// have been received after the last command.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn discard_input_buffer(&self) -> IoResult<()> {
		self.inner.discard_buffers(true, false)
	}

	/// Discard the kernel output buffers for the serial port.
	///
	/// When you write to a serial port, the data is generally put in a buffer
	/// by the OS to be transmitted by the actual device later. This function
	/// clears that buffer: any untransmitted data is discarded by the OS.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn discard_output_buffer(&self) -> IoResult<()> {
		self.inner.discard_buffers(false, true)
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
	/// If we get an error back from the OS.
	pub fn set_rts(&self, state: bool) -> IoResult<()> {
		self.inner.set_rts(state)
	}

	/// Read the state of the Clear To Send line.
	///
	/// If hardware flow control is enabled on the serial port, it is platform
	/// specific what will happen. The function may fail with an error, it may
	/// return a bogus value, or it may return the actual state of the CTS line.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn read_cts(&self) -> IoResult<bool> {
		self.inner.read_cts()
	}

	/// Set the state of the Data Terminal Ready line.
	///
	/// If hardware flow control is enabled on the serial port, it is platform
	/// specific what will happen. The function may fail with an error or it
	/// may silently be ignored.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn set_dtr(&self, state: bool) -> IoResult<()> {
		self.inner.set_dtr(state)
	}

	/// Read the state of the Data Set Ready line.
	///
	/// If hardware flow control is enabled on the serial port, it is platform
	/// specific what will happen. The function may fail with an error, it may
	/// return a bogus value, or it may return the actual state of the DSR line.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn read_dsr(&self) -> IoResult<bool> {
		self.inner.read_dsr()
	}

	/// Read the state of the Ring Indicator line.
	///
	/// This line is also sometimes also called the RNG or RING line.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn read_ri(&self) -> IoResult<bool> {
		self.inner.read_ri()
	}

	/// Read the state of the Carrier Detect (CD) line.
	///
	/// This line is also called the Data Carrier Detect (DCD) line
	/// or the Receive Line Signal Detect (RLSD) line.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS.
	pub fn read_cd(&self) -> IoResult<bool> {
		self.inner.read_cd()
	}
}

impl Read for SyncSerialPort {
	fn read(&mut self, buff: &mut [u8]) -> IoResult<usize> {
		SyncSerialPort::read(self, buff)
	}
	fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> IoResult<usize> {
		SyncSerialPort::read_vectored(self, bufs)
	}
}

impl Read for &'_ SyncSerialPort {
	fn read(&mut self, buff: &mut [u8]) -> IoResult<usize> {
		SyncSerialPort::read(self, buff)
	}
	fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> IoResult<usize> {
		SyncSerialPort::read_vectored(self, bufs)
	}
}

impl Write for SyncSerialPort {
	fn write(&mut self, buff: &[u8]) -> IoResult<usize> {
		SyncSerialPort::write(self, buff)
	}

	fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> IoResult<usize> {
		SyncSerialPort::write_vectored(self, bufs)
	}

	fn flush(&mut self) -> IoResult<()> {
		SyncSerialPort::flush(self)
	}
}

impl Write for &'_ SyncSerialPort {
	fn write(&mut self, buff: &[u8]) -> IoResult<usize> {
		SyncSerialPort::write(self, buff)
	}

	fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> IoResult<usize> {
		SyncSerialPort::write_vectored(self, bufs)
	}

	fn flush(&mut self) -> IoResult<()> {
		SyncSerialPort::flush(self)
	}
}

#[cfg(unix)]
impl From<SyncSerialPort> for std::os::unix::io::OwnedFd {
	fn from(value: SyncSerialPort) -> Self {
		value.inner.fd.into()
	}
}

#[cfg(unix)]
impl From<std::os::unix::io::OwnedFd> for SyncSerialPort {
	fn from(value: std::os::unix::io::OwnedFd) -> Self {
		Self {
			inner: RawSyncSerialPort {
				fd: value.into(),
				read_timeout_ms: DEFAULT_TIMEOUT_MS,
				write_timeout_ms: DEFAULT_TIMEOUT_MS,
			},
		}
	}
}

#[cfg(unix)]
impl std::os::unix::io::AsFd for SyncSerialPort {
	fn as_fd(&self) -> std::os::unix::prelude::BorrowedFd<'_> {
		self.inner.fd.as_fd()
	}
}

#[cfg(unix)]
impl std::os::unix::io::AsRawFd for SyncSerialPort {
	fn as_raw_fd(&self) -> std::os::unix::prelude::RawFd {
		self.inner.fd.as_raw_fd()
	}
}

#[cfg(unix)]
impl std::os::unix::io::IntoRawFd for SyncSerialPort {
	fn into_raw_fd(self) -> std::os::unix::prelude::RawFd {
		self.inner.fd.into_raw_fd()
	}
}

#[cfg(unix)]
impl std::os::unix::io::FromRawFd for SyncSerialPort {
	unsafe fn from_raw_fd(fd: std::os::unix::prelude::RawFd) -> Self {
		Self {
			inner: RawSyncSerialPort {
				fd: std::fs::File::from_raw_fd(fd),
				read_timeout_ms: DEFAULT_TIMEOUT_MS,
				write_timeout_ms: DEFAULT_TIMEOUT_MS,
			},
		}
	}
}

#[cfg(target_os = "windows")]
impl From<SyncSerialPort> for std::os::windows::io::OwnedHandle {
	fn from(value: SyncSerialPort) -> Self {
		value.inner.fd.into()
	}
}

#[cfg(target_os = "windows")]
impl From<std::os::windows::io::OwnedHandle> for SyncSerialPort {
	fn from(value: std::os::windows::io::OwnedHandle) -> Self {
		Self {
			inner: RawSyncSerialPort { fd: value.into() },
		}
	}
}

#[cfg(target_os = "windows")]
impl std::os::windows::io::AsHandle for SyncSerialPort {
	fn as_handle(&self) -> std::os::windows::io::BorrowedHandle<'_> {
		self.inner.fd.as_handle()
	}
}

#[cfg(target_os = "windows")]
impl std::os::windows::io::AsRawHandle for SyncSerialPort {
	fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
		self.inner.fd.as_raw_handle()
	}
}

#[cfg(target_os = "windows")]
impl std::os::windows::io::IntoRawHandle for SyncSerialPort {
	fn into_raw_handle(self) -> std::os::windows::io::RawHandle {
		self.inner.fd.into_raw_handle()
	}
}

#[cfg(target_os = "windows")]
impl std::os::windows::io::FromRawHandle for SyncSerialPort {
	unsafe fn from_raw_handle(handle: std::os::windows::io::RawHandle) -> Self {
		Self {
			inner: RawSyncSerialPort {
				fd: std::fs::File::from_raw_handle(handle),
			},
		}
	}
}
