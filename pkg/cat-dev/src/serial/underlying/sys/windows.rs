//! Thin wrapper around the OS APIs (Windows) for talking to a serial port
//! synchronously.

use crate::serial::underlying::sys::DEFAULT_TIMEOUT_MS;
use bytes::{Bytes, BytesMut};
use std::{
	ffi::CStr,
	fs::{File, OpenOptions},
	io::{Error as IoError, ErrorKind as IoErrorKind, IoSlice, IoSliceMut, Result as IoResult},
	os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
	path::{Path, PathBuf},
	time::Duration,
};
use windows::{
	core::{PCSTR, PSTR},
	Win32::{
		Devices::Communication::{
			EscapeCommFunction, GetCommModemStatus, GetCommTimeouts, PurgeComm, SetCommState,
			SetCommTimeouts, CLRDTR, CLRRTS, COMMTIMEOUTS, DCB, MODEM_STATUS_FLAGS, MS_CTS_ON,
			MS_DSR_ON, MS_RING_ON, MS_RLSD_ON, NOPARITY, ONESTOPBIT, PURGE_COMM_FLAGS,
			PURGE_RXCLEAR, PURGE_TXCLEAR, SETDTR, SETRTS,
		},
		Foundation::{CloseHandle, ERROR_IO_PENDING, ERROR_NO_MORE_ITEMS, HANDLE},
		Storage::FileSystem::{FlushFileBuffers, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED},
		System::{
			Registry::{
				RegCloseKey, RegEnumValueA, RegOpenKeyExA, RegQueryInfoKeyA, HKEY,
				HKEY_LOCAL_MACHINE, KEY_READ, REG_SAM_FLAGS, REG_SZ,
			},
			Threading::CreateEventA,
			IO::{GetOverlappedResult, OVERLAPPED},
		},
	},
};

#[derive(Debug)]
pub struct RawSyncSerialPort {
	/// The file descriptor to talk to this serial port on.
	pub fd: File,
}

impl RawSyncSerialPort {
	/// Create a new connection to a serial port.
	///
	/// The path you pass in for windows should be something like "COM1", "COM2",
	/// we will implicitly add in: `\\.\` to whatever you pass in as your path.
	///
	/// ## Errors
	///
	/// - If we cannot open the chosen serial port.
	/// - If we cannot set the timeouts on the serial port.
	/// - If we cannot set the comm state to that of what we expect for a
	///   cat-dev.
	#[allow(
		// Guaranteed to not truncate.
		clippy::cast_possible_truncation,
	)]
	pub fn new(path: impl AsRef<Path>) -> IoResult<Self> {
		// Use the win32 device namespace, otherwise we're limited to COM1-9.
		//
		// <https://docs.microsoft.com/en-us/windows/win32/fileio/naming-a-file#win32-device-namespaces>
		let mut serial_port_path = PathBuf::from(r"\\.");
		serial_port_path.push(path);

		let fd = OpenOptions::new()
			.read(true)
			.write(true)
			.create(false)
			.custom_flags(FILE_FLAG_OVERLAPPED.0)
			.open(&serial_port_path)?;

		let timeouts = COMMTIMEOUTS {
			ReadIntervalTimeout: u32::MAX,
			ReadTotalTimeoutMultiplier: u32::MAX,
			ReadTotalTimeoutConstant: DEFAULT_TIMEOUT_MS,
			WriteTotalTimeoutMultiplier: 0,
			WriteTotalTimeoutConstant: DEFAULT_TIMEOUT_MS,
		};
		unsafe {
			SetCommTimeouts(HANDLE(fd.as_raw_handle() as isize), &timeouts)
				.map_err(|_| IoError::last_os_error())?;
		}
		let dcb = DCB {
			DCBlength: std::mem::size_of::<DCB>() as u32,
			BaudRate: 57600,
			ByteSize: 8,
			Parity: NOPARITY,
			StopBits: ONESTOPBIT,
			..Default::default()
		};
		unsafe {
			SetCommState(HANDLE(fd.as_raw_handle() as isize), &dcb)
				.map_err(|_| IoError::last_os_error())?;
		}

		Ok(Self { fd })
	}

	/// Attempt to clone this particular object.
	///
	/// ## Errors
	///
	/// If we cannot end up cloning the file descriptor.
	pub fn try_clone(&self) -> IoResult<Self> {
		Ok(Self {
			fd: self.fd.try_clone()?,
		})
	}

	/// Get the read timeout for this serial port.
	///
	/// ## Errors
	///
	/// If we cannot communicate with the port to get it's configured timeouts.
	pub fn get_read_timeout(&self) -> IoResult<Duration> {
		let mut timeouts = unsafe { std::mem::zeroed() };
		unsafe {
			GetCommTimeouts(HANDLE(self.fd.as_raw_handle() as isize), &mut timeouts)
				.map_err(|_| IoError::last_os_error())?;
		}
		Ok(Duration::from_millis(
			timeouts.ReadTotalTimeoutConstant.into(),
		))
	}

	/// Get the write timeout for this serial port.
	///
	/// ## Errors
	///
	/// If we cannot communicate with the port to get it's configured timeouts.
	pub fn get_write_timeout(&self) -> IoResult<Duration> {
		let mut timeouts = unsafe { std::mem::zeroed() };
		unsafe {
			GetCommTimeouts(HANDLE(self.fd.as_raw_handle() as isize), &mut timeouts)
				.map_err(|_| IoError::last_os_error())?;
		}
		Ok(Duration::from_millis(
			timeouts.WriteTotalTimeoutConstant.into(),
		))
	}

	/// Set the read timeout for this serial port.
	///
	/// ## Errors
	///
	/// If the timeout is not between 1, or 4294967294 millisecds (aka 1ms,
	/// and ~50 days). Or if we cannot get/set the timeouts when talking to
	/// the serial port.
	pub fn set_read_timeout(&self, new_timeout: Duration) -> IoResult<()> {
		let timeout_ms = new_timeout.as_millis();
		if timeout_ms < 1 || timeout_ms > (u32::MAX - 1).into() {
			return Err(IoError::other(
				"read timeout must be between 1, and 4294967294",
			));
		}

		let mut timeouts = unsafe { std::mem::zeroed() };
		unsafe {
			GetCommTimeouts(HANDLE(self.fd.as_raw_handle() as isize), &mut timeouts)
				.map_err(|_| IoError::last_os_error())?;
		}
		timeouts.ReadIntervalTimeout = u32::MAX;
		timeouts.ReadTotalTimeoutMultiplier = u32::MAX;
		timeouts.ReadTotalTimeoutConstant = timeout_ms.try_into().unwrap_or(u32::MAX);
		unsafe {
			SetCommTimeouts(HANDLE(self.fd.as_raw_handle() as isize), &timeouts)
				.map_err(|_| IoError::last_os_error())?;
		}

		Ok(())
	}

	/// Set the write timeout for this serial port.
	///
	/// ## Errors
	///
	/// If the timeout is not between 1, or 4294967294 millisecds (aka 1ms,
	/// and ~50 days). Or if we cannot get/set the timeouts when talking to
	/// the serial port.
	pub fn set_write_timeout(&self, new_timeout: Duration) -> IoResult<()> {
		let timeout_ms = new_timeout.as_millis();
		if timeout_ms < 1 || timeout_ms > (u32::MAX - 1).into() {
			return Err(IoError::other(
				"write timeout must be between 1, and 4294967294",
			));
		}

		let mut timeouts = unsafe { std::mem::zeroed() };
		unsafe {
			GetCommTimeouts(HANDLE(self.fd.as_raw_handle() as isize), &mut timeouts)
				.map_err(|_| IoError::last_os_error())?;
		}
		timeouts.WriteTotalTimeoutMultiplier = u32::MAX;
		timeouts.WriteTotalTimeoutConstant = timeout_ms.try_into().unwrap_or(u32::MAX);
		unsafe {
			SetCommTimeouts(HANDLE(self.fd.as_raw_handle() as isize), &timeouts)
				.map_err(|_| IoError::last_os_error())?;
		}

		Ok(())
	}

	/// Attempt to read data into a buffer from the serial port.
	///
	/// ## Errors
	///
	/// If we get an error from the OS attempting to read bytes from the OS.
	#[allow(
		// Wrap is guaranteed to not happen in this error code.
		clippy::cast_possible_wrap,
	)]
	pub fn read(&self, buff: &mut [u8]) -> IoResult<usize> {
		let event = Event::create(false, false)?;
		let mut read_bytes = 0_u32;
		let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
		overlapped.hEvent = event.handle;

		match unsafe {
			ReadFile(
				HANDLE(self.fd.as_raw_handle() as isize),
				Some(buff),
				Some(&mut read_bytes),
				Some(&mut overlapped),
			)
		}
		.map_err(|_| IoError::last_os_error())
		{
			// Windows reports timeouts as a successful transfer of 0 bytes.
			Ok(()) => {
				if read_bytes == 0 {
					Err(IoErrorKind::TimedOut.into())
				} else {
					Ok(read_bytes as usize)
				}
			}
			// BrokenPipe with reads means EOF on Windows.
			Err(cause) => {
				if cause.kind() == IoErrorKind::BrokenPipe {
					Ok(0)
				} else if cause.raw_os_error() == Some(ERROR_IO_PENDING.0 as i32) {
					Self::wait_async_transfer(&self.fd, &mut overlapped).or_else(|error| {
						if error.kind() == IoErrorKind::BrokenPipe {
							Ok(0)
						} else {
							Err(error)
						}
					})
				} else {
					Err(cause)
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
	/// Always, unfortunately windows does not support vectord reads.
	#[allow(
		// Need to match API with linux which does use self.
		clippy::unused_self,
	)]
	pub fn read_vectored(&self, _bufs: &mut [IoSliceMut<'_>]) -> IoResult<usize> {
		Err(IoError::from_raw_os_error(
			windows::Win32::Networking::WinSock::WSAENOTSOCK.0,
		))
	}

	/// Attempt to write data from a buffer to the serial port.
	///
	/// ## Errors
	///
	/// If we get an error from the OS attempting to write bytes from the OS.
	#[allow(
		// Wrap is guaranteed to not happen for an error code in this case.
		clippy::cast_possible_wrap,
	)]
	pub fn write(&self, buff: &[u8]) -> IoResult<usize> {
		let event = Event::create(false, false)?;
		let mut written = 0;
		let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
		overlapped.hEvent = event.handle;

		match unsafe {
			WriteFile(
				HANDLE(self.fd.as_raw_handle() as isize),
				Some(buff),
				Some(&mut written),
				Some(&mut overlapped),
			)
		}
		.map_err(|_| IoError::last_os_error())
		{
			// Windows reports timeouts as a succesfull transfer of 0 bytes.
			Ok(()) => {
				if written == 0 {
					Err(IoErrorKind::TimedOut.into())
				} else {
					Ok(written as usize)
				}
			}
			Err(cause) => {
				if cause.raw_os_error() == Some(ERROR_IO_PENDING.0 as i32) {
					Self::wait_async_transfer(&self.fd, &mut overlapped)
				} else {
					Err(cause)
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
	/// Always, unfortunately windows does not support vectord writes.
	#[allow(
		// Need to match API with linux which does use self.
		clippy::unused_self,
	)]
	pub fn write_vectored(&self, _bufs: &[IoSlice<'_>]) -> IoResult<usize> {
		Err(IoError::from_raw_os_error(
			windows::Win32::Networking::WinSock::WSAENOTSOCK.0,
		))
	}

	/// Flush all output to the serial port.
	///
	/// ## Errors
	///
	/// If we cannot call `FlushFileBuffers`, or it returns an error.
	pub fn flush_output(&self) -> IoResult<()> {
		unsafe {
			FlushFileBuffers(HANDLE(self.fd.as_raw_handle() as isize))
				.map_err(|_| IoError::last_os_error())
		}
	}

	/// Attempt to discard any existing buffers.
	///
	/// ## Errors
	///
	/// If `PurgeComm` returns an error from the OS.
	pub fn discard_buffers(&self, discard_input: bool, discard_output: bool) -> IoResult<()> {
		let mut flags = 0;
		if discard_input {
			flags |= PURGE_RXCLEAR.0;
		}
		if discard_output {
			flags |= PURGE_TXCLEAR.0;
		}

		unsafe {
			PurgeComm(
				HANDLE(self.fd.as_raw_handle() as isize),
				PURGE_COMM_FLAGS(flags),
			)
			.map_err(|_| IoError::last_os_error())
		}
	}

	/// Set the (request-to-send) signal to on or off.
	///
	/// ## Errors
	///
	/// If we get an error calling `EscapeCommFunction`.
	pub fn set_rts(&self, state: bool) -> IoResult<()> {
		unsafe {
			EscapeCommFunction(
				HANDLE(self.fd.as_raw_handle() as isize),
				if state { SETRTS } else { CLRRTS },
			)
			.map_err(|_| IoError::last_os_error())
		}
	}

	/// Check the clear-to-send signal of a serial device.
	///
	/// ## Errors
	///
	/// If we cannot call the underlying OS APIs.
	pub fn read_cts(&self) -> IoResult<bool> {
		Self::read_pin(&self.fd, MS_CTS_ON.0)
	}

	/// Set the (data-terminal-ready) signal.
	///
	/// ## Errors
	///
	/// If we cannot call the underlying OS APIs.
	pub fn set_dtr(&self, state: bool) -> IoResult<()> {
		unsafe {
			EscapeCommFunction(
				HANDLE(self.fd.as_raw_handle() as isize),
				if state { SETDTR } else { CLRDTR },
			)
			.map_err(|_| IoError::last_os_error())
		}
	}

	/// Check the data-set-ready signal of a serial device.
	///
	/// ## Errors
	///
	/// If we cannot call the underlying OS APIs.
	pub fn read_dsr(&self) -> IoResult<bool> {
		Self::read_pin(&self.fd, MS_DSR_ON.0)
	}

	/// Check the ring indicator signal of a serial device.
	///
	/// ## Errors
	///
	/// If we cannot call the underlying OS APIs.
	pub fn read_ri(&self) -> IoResult<bool> {
		Self::read_pin(&self.fd, MS_RING_ON.0)
	}

	/// Check the carrier detect signal of a serial device.
	///
	/// ## Errors
	///
	/// If we cannot call the underlying OS APIs.
	pub fn read_cd(&self) -> IoResult<bool> {
		Self::read_pin(&self.fd, MS_RLSD_ON.0)
	}

	/// Enumerate all possible serial devices.
	///
	/// ## Errors
	///
	/// If we cannot query the registry for enumerating all possible serial devices.
	pub fn enumerate() -> IoResult<Vec<PathBuf>> {
		let subkey =
			unsafe { CStr::from_bytes_with_nul_unchecked(b"Hardware\\DEVICEMAP\\SERIALCOMM\x00") };
		let device_map = match RegKey::open(HKEY_LOCAL_MACHINE, subkey, KEY_READ) {
			Ok(map) => map,
			Err(cause) => {
				if cause.kind() == IoErrorKind::NotFound {
					return Ok(Vec::with_capacity(0));
				}

				return Err(cause);
			}
		};

		let (value_count, max_value_name_len, max_value_data_len) = device_map.get_value_info()?;

		let mut entries = Vec::with_capacity(16);
		for idx in 0..value_count {
			let Ok(Some((_, mut name))) =
				device_map.get_string_value(idx, max_value_name_len, max_value_data_len)
			else {
				continue;
			};

			if let Some(nul_byte_idx) = name.iter().rposition(|&b| b != 0) {
				name.truncate(nul_byte_idx + 1);
				if let Ok(name) = String::from_utf8(name.to_vec()) {
					entries.push(name.into());
				}
			}
		}

		Ok(entries)
	}

	/// Attempt to read a pin from a serial device.
	///
	/// ## Errors
	///
	/// If we get an error back from `GetCommModemStatus`.
	fn read_pin(fd: &File, pin: u32) -> IoResult<bool> {
		let mut bits: MODEM_STATUS_FLAGS = MODEM_STATUS_FLAGS(0);
		unsafe {
			GetCommModemStatus(HANDLE(fd.as_raw_handle() as isize), &mut bits)
				.map_err(|_| IoError::last_os_error())?;
		}
		Ok(bits.0 & pin != 0)
	}

	fn wait_async_transfer(file: &File, overlapped: &mut OVERLAPPED) -> IoResult<usize> {
		unsafe {
			let mut transferred = 0;

			match GetOverlappedResult(
				HANDLE(file.as_raw_handle() as isize),
				overlapped,
				&mut transferred,
				true,
			)
			.map_err(|_| IoError::last_os_error())
			{
				// Windows reports timeouts as a succesfull transfer of 0 bytes.
				Ok(()) if transferred == 0 => Err(IoErrorKind::TimedOut.into()),
				Ok(()) => Ok(transferred as usize),
				Err(e) => Err(e),
			}
		}
	}
}

struct Event {
	handle: HANDLE,
}
impl Event {
	fn create(manual_reset: bool, initially_signalled: bool) -> IoResult<Self> {
		let handle = unsafe {
			CreateEventA(
				None, // security attributes
				manual_reset,
				initially_signalled,
				PCSTR(std::ptr::null()), // name
			)
			.map_err(|_| IoError::last_os_error())?
		};

		Ok(Self { handle })
	}
}
impl Drop for Event {
	fn drop(&mut self) {
		unsafe {
			std::mem::drop(CloseHandle(self.handle));
		}
	}
}

#[derive(Debug)]
struct RegKey {
	key: HKEY,
}
impl RegKey {
	fn open(parent: HKEY, subpath: &CStr, rights: REG_SAM_FLAGS) -> IoResult<Self> {
		let mut key: HKEY = HKEY(std::ptr::null_mut::<std::ffi::c_void>() as isize);

		unsafe {
			RegOpenKeyExA(parent, PCSTR(subpath.as_ptr().cast()), 0, rights, &mut key)
				// Yes this is what gets us an actual result, :eyeroll:
				.ok()
				.map_err(|_| IoError::last_os_error())?;
		}

		Ok(Self { key })
	}

	fn get_value_info(&self) -> IoResult<(u32, u32, u32)> {
		let mut value_count = 0_u32;
		let mut max_value_name_len = 0_u32;
		let mut max_value_data_len = 0_u32;

		unsafe {
			RegQueryInfoKeyA(
				self.key,
				PSTR::null(),
				None,
				None,
				None,
				None,
				None,
				Some(&mut value_count),
				Some(&mut max_value_name_len),
				Some(&mut max_value_data_len),
				None,
				None,
			)
			.ok()
		}
		.map_err(|_| IoError::last_os_error())?;

		Ok((value_count, max_value_name_len, max_value_data_len))
	}

	#[allow(
		// Truncation is guaranteed to not happen in this context.
		clippy::cast_possible_truncation,
	)]
	fn get_string_value(
		&self,
		index: u32,
		max_name_len: u32,
		max_data_len: u32,
	) -> IoResult<Option<(Bytes, Bytes)>> {
		let mut name = BytesMut::zeroed(max_name_len as usize + 1);
		let mut data = BytesMut::zeroed(max_data_len as usize);
		let mut name_len = name.len() as u32;
		let mut data_len = data.len() as u32;
		let mut kind = 0;

		let result = unsafe {
			RegEnumValueA(
				self.key,
				index,
				PSTR::from_raw(name.as_mut_ptr().cast()),
				&mut name_len,
				None,
				Some(&mut kind),
				Some(data.as_mut_ptr().cast()),
				Some(&mut data_len),
			)
		};
		// Yes '.ok()' returns a result type here, this is such a weird api.
		if let Err(cause) = result.ok() {
			if cause.code() == ERROR_NO_MORE_ITEMS.into() {
				Ok(None)
			} else {
				Err(IoError::from_raw_os_error(cause.code().0))
			}
		} else if kind != REG_SZ.0 {
			Ok(None)
		} else {
			name.truncate(name_len as usize + 1);
			data.truncate(data_len as usize);
			Ok(Some((name.freeze(), data.freeze())))
		}
	}
}
impl Drop for RegKey {
	fn drop(&mut self) {
		unsafe {
			_ = RegCloseKey(self.key);
		}
	}
}
