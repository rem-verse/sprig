use crate::serial::underlying::sys::DEFAULT_TIMEOUT_MS;
use libc::{O_NOCTTY, O_NONBLOCK};
use std::{
	fs::{File, OpenOptions},
	io::{Error as IoError, ErrorKind as IoErrorKind, IoSlice, IoSliceMut, Result as IoResult},
	os::{
		raw::{c_int, c_short},
		unix::{fs::OpenOptionsExt, io::AsRawFd},
	},
	path::{Path, PathBuf},
	time::Duration,
};

#[cfg(all(
	any(target_os = "android", target_os = "linux"),
	not(any(target_arch = "powerpc", target_arch = "powerpc64"))
))]
pub type RawTermios = libc::termios2;
#[cfg(not(all(
	any(target_os = "android", target_os = "linux"),
	not(any(target_arch = "powerpc", target_arch = "powerpc64"))
)))]
pub type RawTermios = libc::termios;

#[derive(Debug)]
pub struct RawSyncSerialPort {
	pub fd: File,
	pub read_timeout_ms: u32,
	pub write_timeout_ms: u32,
}

impl RawSyncSerialPort {
	/// Attempt to connect to a new raw serial port.
	///
	/// ## Errors
	///
	/// If we cannot open the file to the serial port device, or if we cannot
	/// configure it appropriately for talking with a cat-dev device.
	pub fn new(path: impl AsRef<Path>) -> IoResult<Self> {
		let this = Self {
			fd: OpenOptions::new()
				.read(true)
				.write(true)
				.create(false)
				.custom_flags(O_NONBLOCK | O_NOCTTY)
				.open(path)?,
			read_timeout_ms: DEFAULT_TIMEOUT_MS,
			write_timeout_ms: DEFAULT_TIMEOUT_MS,
		};

		let mut termios = Self::get_termios_from_fd(&this.fd)?;
		Self::set_baud_rate(&mut termios)?;
		termios.c_cflag = (termios.c_cflag & !libc::CSIZE) | libc::CS8;
		termios.c_cflag = termios.c_cflag & !libc::PARODD & !libc::PARENB;
		termios.c_cflag &= !libc::CSTOPB;
		Self::set_termios_on_fd(&this.fd, &termios)?;

		Ok(this)
	}

	/// Attempt to clone this particular object.
	///
	/// ## Errors
	///
	/// If we cannot end up cloning the file descriptor.
	pub fn try_clone(&self) -> IoResult<Self> {
		Ok(Self {
			fd: self.fd.try_clone()?,
			read_timeout_ms: self.read_timeout_ms,
			write_timeout_ms: self.write_timeout_ms,
		})
	}

	/// Get the read timeout for this serial port.
	///
	/// ## Errors
	///
	/// Never, used for API compatability with windows which can fail.
	#[allow(
		// Windows needs a result type here, and we want to match signatures.
		clippy::unnecessary_wraps,
	)]
	pub fn get_read_timeout(&self) -> IoResult<Duration> {
		Ok(Duration::from_millis(self.read_timeout_ms.into()))
	}

	/// Get the write timeout for this serial port.
	///
	/// ## Errors
	///
	/// Never, used for API compatability with windows which can fail.
	#[allow(
		// Windows needs a result type here, and we want to match signatures.
		clippy::unnecessary_wraps,
	)]
	pub fn get_write_timeout(&self) -> IoResult<Duration> {
		Ok(Duration::from_millis(self.write_timeout_ms.into()))
	}

	/// Set the read timeout for this serial port.
	///
	/// ## Errors
	///
	/// If the timeout is not between 1, or 4294967294 millisecds (aka 1ms,
	/// and ~50 days).
	pub fn set_read_timeout(&mut self, new_timeout: Duration) -> IoResult<()> {
		let timeout_ms = new_timeout.as_millis();
		if timeout_ms < 1 || timeout_ms > (u32::MAX - 1).into() {
			return Err(IoError::other(
				"read timeout must be between 1, and 4294967294",
			));
		}
		self.read_timeout_ms = timeout_ms.try_into().unwrap_or(u32::MAX);
		Ok(())
	}

	/// Set the write timeout for this serial port.
	///
	/// ## Errors
	///
	/// If the timeout is not between 1, or 4294967294 millisecds (aka 1ms,
	/// and ~50 days).
	pub fn set_write_timeout(&mut self, new_timeout: Duration) -> IoResult<()> {
		let timeout_ms = new_timeout.as_millis();
		if timeout_ms < 1 || timeout_ms > (u32::MAX - 1).into() {
			return Err(IoError::other(
				"write timeout must be between 1, and 4294967294",
			));
		}
		self.write_timeout_ms = timeout_ms.try_into().unwrap_or(u32::MAX);
		Ok(())
	}

	/// Attempt to read data into a buffer from the serial port.
	///
	/// ## Errors
	///
	/// If we get an error from the OS attempting to read bytes from the OS.
	#[allow(
		// We manually check that sign loss isn't an issue.
		clippy::cast_sign_loss,
	)]
	pub fn read(&self, buff: &mut [u8]) -> IoResult<usize> {
		if !Self::poll(&self.fd, libc::POLLIN, self.read_timeout_ms)? {
			return Err(IoErrorKind::TimedOut.into());
		}

		loop {
			match unsafe {
				Self::check_isize(libc::read(
					self.fd.as_raw_fd(),
					buff.as_mut_ptr().cast(),
					buff.len() as _,
				))
			} {
				Ok(size) => return Ok(size as usize),
				Err(cause) => {
					if cause.raw_os_error() == Some(libc::EINTR) {
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
		true
	}

	/// Attempt to read from multiple ranges at once, aka perform a "vectored"
	/// read.
	///
	/// ## Errors
	///
	/// If we timeout waiting for the device to become ready, or if we cannot
	/// execute the `readv` system call.
	#[allow(
		// Truncation is okay in this api.
		clippy::cast_possible_truncation,
		// Wrapping is okay in this API.
		clippy::cast_possible_wrap,
		// We manually check to ensure sign loss isn't a big deal.
		clippy::cast_sign_loss,
	)]
	pub fn read_vectored(&self, buff: &mut [IoSliceMut<'_>]) -> IoResult<usize> {
		if !Self::poll(&self.fd, libc::POLLIN, self.read_timeout_ms)? {
			return Err(IoErrorKind::TimedOut.into());
		}

		loop {
			match unsafe {
				Self::check_isize(libc::readv(
					self.fd.as_raw_fd(),
					buff.as_mut_ptr().cast(),
					buff.len() as _,
				))
			} {
				Ok(read) => return Ok(read as usize),
				Err(cause) => {
					if cause.raw_os_error() == Some(libc::EINTR) {
						continue;
					}

					return Err(cause);
				}
			}
		}
	}

	/// Attempt to write data from a buffer to the serial port.
	///
	/// ## Errors
	///
	/// If we get an error from the OS attempting to write bytes from the OS.
	#[allow(
		// Truncation is okay for this API.
		clippy::cast_possible_truncation,
		// Wrapping is okay for this API.
		clippy::cast_possible_wrap,
		// We validate manually the sign loss isn't an issue.
		clippy::cast_sign_loss,
	)]
	pub fn write(&self, buff: &[u8]) -> IoResult<usize> {
		if !Self::poll(&self.fd, libc::POLLOUT, self.write_timeout_ms)? {
			return Err(IoErrorKind::TimedOut.into());
		}

		loop {
			match unsafe {
				Self::check_isize(libc::write(
					self.fd.as_raw_fd(),
					buff.as_ptr().cast(),
					buff.len() as _,
				))
			} {
				Ok(size) => return Ok(size as usize),
				Err(cause) => {
					if cause.raw_os_error() == Some(libc::EINTR) {
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
		true
	}

	/// Attempt to write to multiple ranges at once, aka perform a "vectored"
	/// write.
	///
	/// ## Errors
	///
	/// If we timeout waiting for the device to become ready, or if we cannot
	/// execute the `writev` system call.
	#[allow(
		// Wrapping is okay for this API.
		clippy::cast_possible_wrap,
		// Truncation is okay in this API.
		clippy::cast_possible_truncation,
		// We validate sign loss isn't an issue manually.
		clippy::cast_sign_loss,
	)]
	pub fn write_vectored(&self, buff: &[IoSlice<'_>]) -> IoResult<usize> {
		if !Self::poll(&self.fd, libc::POLLOUT, self.write_timeout_ms)? {
			return Err(IoErrorKind::TimedOut.into());
		}

		loop {
			match unsafe {
				Self::check_isize(libc::writev(
					self.fd.as_raw_fd(),
					buff.as_ptr().cast(),
					buff.len() as _,
				))
			} {
				Ok(read) => return Ok(read as usize),
				Err(cause) => {
					if cause.raw_os_error() == Some(libc::EINTR) {
						continue;
					}

					return Err(cause);
				}
			}
		}
	}

	/// Flush all output to the serial port.
	///
	/// ## Errors
	///
	/// If we cannot call `tcdrain`, or it returns an error.
	pub fn flush_output(&self) -> IoResult<()> {
		unsafe {
			Self::check(libc::tcdrain(self.fd.as_raw_fd()))?;
		}

		Ok(())
	}

	/// Attempt to discard any existing buffers.
	///
	/// ## Errors
	///
	/// If `tcflush` returns an error from the OS.
	pub fn discard_buffers(&self, discard_input: bool, discard_output: bool) -> IoResult<()> {
		let mut flags = 0;
		if discard_input {
			flags |= libc::TCIFLUSH;
		}
		if discard_output {
			flags |= libc::TCOFLUSH;
		}

		unsafe {
			Self::check(libc::tcflush(self.fd.as_raw_fd(), flags))?;
		}

		Ok(())
	}

	/// Set the (request-to-send) signal to on or off.
	///
	/// ## Errors
	///
	/// If we get an error calling `EscapeCommFunction`.
	pub fn set_rts(&self, state: bool) -> IoResult<()> {
		Self::set_pin(&self.fd, libc::TIOCM_RTS, state)
	}

	/// Check the clear-to-send signal of a serial device.
	///
	/// ## Errors
	///
	/// If we cannot call the underlying OS APIs.
	pub fn read_cts(&self) -> IoResult<bool> {
		Self::read_pin(&self.fd, libc::TIOCM_CTS)
	}

	/// Set the (data-terminal-ready) signal.
	///
	/// ## Errors
	///
	/// If we cannot call the underlying OS APIs.
	pub fn set_dtr(&self, state: bool) -> IoResult<()> {
		Self::set_pin(&self.fd, libc::TIOCM_DTR, state)
	}

	/// Check the data-set-ready signal of a serial device.
	///
	/// ## Errors
	///
	/// If we cannot call the underlying OS APIs.
	pub fn read_dsr(&self) -> IoResult<bool> {
		Self::read_pin(&self.fd, libc::TIOCM_DSR)
	}

	/// Check the ring indicator signal of a serial device.
	///
	/// ## Errors
	///
	/// If we cannot call the underlying OS APIs.
	pub fn read_ri(&self) -> IoResult<bool> {
		Self::read_pin(&self.fd, libc::TIOCM_RI)
	}

	/// Check the carrier detect signal of a serial device.
	///
	/// ## Errors
	///
	/// If we cannot call the underlying OS APIs.
	pub fn read_cd(&self) -> IoResult<bool> {
		Self::read_pin(&self.fd, libc::TIOCM_CD)
	}

	/// Enumerate all possible serial devices.
	///
	/// ## Errors
	///
	/// If we cannot look at the OS for the possible OS devices.
	#[cfg(any(
		target_os = "dragonfly",
		target_os = "freebsd",
		target_os = "ios",
		target_os = "macos",
		target_os = "netbsd",
		target_os = "openbsd",
	))]
	pub fn enumerate() -> IoResult<Vec<PathBuf>> {
		use std::os::unix::{ffi::OsStrExt, fs::FileTypeExt};

		Ok(std::fs::read_dir("/dev")?
			.filter_map(|resulting_entry| {
				let entry = resulting_entry.ok()?;
				let kind = entry.metadata().ok()?.file_type();
				if kind.is_char_device() && Self::is_tty_name(entry.file_name().as_bytes()) {
					Some(entry.path())
				} else {
					None
				}
			})
			.collect())
	}

	#[cfg(any(target_os = "linux", target_os = "android"))]
	pub fn enumerate() -> IoResult<Vec<PathBuf>> {
		use std::os::unix::{ffi::OsStrExt, fs::FileTypeExt};

		Ok(std::fs::read_dir("/sys/class/tty")?
			.filter_map(|resulting_entry| {
				let entry = resulting_entry.ok()?;
				let base_name = entry.file_name();

				let dev_path = Path::new("/dev").join(&base_name);
				let metadata = dev_path.metadata().ok()?;
				if !metadata.file_type().is_char_device() {
					return None;
				}

				match base_name.as_bytes().strip_prefix(b"tty") {
					// Skip entries just called "tty", or things not starting with "tty",
					// probably not a serial port
					Some(b"") | None => return None,
					// Skip "tty1", "tty2", etc (these these are virtual terminals,
					// not serial ports).
					Some(&[c, ..]) if c.is_ascii_digit() => return None,
					// Accept the rest.
					Some(_) => (),
				};

				// There's a bunch of ttyS* ports that are not really serial ports.
				//
				// They have a file called `device/driver_override` set to "(null)".
				if let Ok(driver_override) =
					std::fs::read(entry.path().join("device/driver_override"))
				{
					if driver_override == b"(null)\n" {
						return None;
					}
				}

				Some(dev_path)
			})
			.collect())
	}

	#[cfg(any(target_os = "illumos", target_os = "solaris"))]
	pub fn enumerate() -> IoResult<Vec<PathBuf>> {
		use std::os::unix::fs::FileTypeExt;

		// https://illumos.org/man/1M/ports
		// Let's hope Solaris is doing the same.
		// If only Oracle actually had navigatable documentation.
		let cua = std::fs::read_dir("/dev/cua")?;
		let term = std::fs::read_dir("/dev/cua")?;

		Ok(cua
			.chain(term)
			.filter_map(|entry| {
				let entry = entry.ok()?;
				let kind = entry.metadata().ok()?.file_type();
				if kind.is_char_device() {
					Some(entry.path())
				} else {
					None
				}
			})
			.collect())
	}

	#[cfg(not(any(
		target_os = "dragonfly",
		target_os = "freebsd",
		target_os = "ios",
		target_os = "macos",
		target_os = "netbsd",
		target_os = "openbsd",
		target_os = "illumos",
		target_os = "solaris",
		target_os = "linux",
		target_os = "android",
	)))]
	pub fn enumerate() -> IoResult<Vec<PathBuf>> {
		Err(IoError::new(
			IoErrorKind::Other,
			"port enumeration is not implemented for this platform",
		))
	}

	#[cfg(any(target_os = "ios", target_os = "macos"))]
	fn is_tty_name(name: &[u8]) -> bool {
		// Sigh, closed source doesn't have to mean undocumented.
		// Anyway:
		// https://stackoverflow.com/questions/14074413/serial-port-names-on-mac-os-x
		// https://learn.adafruit.com/ftdi-friend/com-slash-serial-port-name
		name.starts_with(b"tty.") || name.starts_with(b"cu.")
	}

	#[cfg(any(
		target_os = "dragonfly",
		target_os = "freebsd",
		target_os = "netbsd",
		target_os = "openbsd",
	))]
	fn is_tty_name(name: &[u8]) -> bool {
		// For BSD variants, we simply report all entries in /dev that look like a TTY.
		// This may contain a lot of false positives for pseudo-terminals or other fake terminals.
		// If anyone can improve this for a specific BSD they love, by all means send a PR.

		// https://man.dragonflybsd.org/?command=sio&section=4
		// https://leaf.dragonflybsd.org/cgi/web-man?command=ucom&section=ANY
		#[cfg(target_os = "dragonfly")]
		const PREFIXES: [&[u8]; 4] = [b"ttyd", b"cuaa", b"ttyU", b"cuaU"];

		// https://www.freebsd.org/cgi/man.cgi?query=uart&sektion=4&apropos=0&manpath=FreeBSD+13.0-RELEASE+and+Ports
		// https://www.freebsd.org/cgi/man.cgi?query=ucom&sektion=4&apropos=0&manpath=FreeBSD+13.0-RELEASE+and+Ports
		#[cfg(target_os = "freebsd")]
		const PREFIXES: [&[u8]; 5] = [b"ttyu", b"cuau", b"cuad", b"ttyU", b"cuaU"];

		// https://man.netbsd.org/com.4
		// https://man.netbsd.org/ucom.4
		#[cfg(target_os = "netbsd")]
		const PREFIXES: [&[u8]; 4] = [b"tty", b"dty", b"ttyU", b"dtyU"];

		// https://man.openbsd.org/com
		// https://man.openbsd.org/ucom
		#[cfg(target_os = "openbsd")]
		const PREFIXES: [&[u8]; 4] = [b"tty", b"cua", b"ttyU", b"cuaU"];

		for prefix in PREFIXES {
			if let Some(suffix) = name.strip_prefix(prefix) {
				if !suffix.is_empty() && suffix.iter().all(|c| c.is_ascii_digit()) {
					return true;
				}
			}
		}

		false
	}

	/// Poll a file descriptor for an event being ready.
	///
	/// ## Errors
	///
	/// If we cannot poll the file descriptor.
	#[allow(
		// Wrapping is okay here.
		clippy::cast_possible_wrap,
	)]
	fn poll(fd: &File, events: c_short, timeout_ms: u32) -> IoResult<bool> {
		let mut poll_fd = libc::pollfd {
			fd: fd.as_raw_fd(),
			events,
			revents: 0,
		};
		unsafe {
			Self::check(libc::poll(&mut poll_fd, 1, timeout_ms as i32))?;
		}
		Ok(poll_fd.revents != 0)
	}

	/// Read the pin of a particular serial device.
	///
	/// ## Errors
	///
	/// If we cannot call `ioctl` on the file descriptor.
	fn read_pin(fd: &File, pin: c_int) -> IoResult<bool> {
		let mut bits: c_int = 0;
		unsafe {
			Self::check(libc::ioctl(fd.as_raw_fd(), libc::TIOCMGET as _, &mut bits))?;
		}
		Ok(bits & pin != 0)
	}

	/// Set the pin of a particular serial device.
	///
	/// ## Errors
	///
	/// If we cannot call `ioctl` on the file descriptor.
	fn set_pin(fd: &File, pin: c_int, state: bool) -> IoResult<()> {
		unsafe {
			Self::check(libc::ioctl(
				fd.as_raw_fd(),
				if state {
					libc::TIOCMBIS
				} else {
					libc::TIOCMBIC
				} as _,
				&pin,
			))?;
		}

		Ok(())
	}

	/// Small wrapper function to perform setting the baud rate flags
	/// appropriately.
	///
	/// ## Errors
	///
	/// If we get an error back from the OS attempting to set some speeds.
	#[allow(
		// Certain paths need the return type to be a result, others do not.
		clippy::unnecessary_wraps,
	)]
	fn set_baud_rate(termios: &mut RawTermios) -> IoResult<()> {
		#[cfg(any(
			target_os = "dragonfly",
			target_os = "freebsd",
			target_os = "ios",
			target_os = "macos",
			target_os = "netbsd",
			target_os = "openbsd",
		))]
		unsafe {
			Self::check(libc::cfsetospeed(termios, 57600 as _))?;
			Self::check(libc::cfsetispeed(termios, 57600 as _))?;
			Ok(())
		}

		#[cfg(all(
			not(any(
				target_os = "dragonfly",
				target_os = "freebsd",
				target_os = "ios",
				target_os = "macos",
				target_os = "netbsd",
				target_os = "openbsd",
			)),
			any(target_os = "android", target_os = "linux"),
			not(any(target_arch = "powerpc", target_arch = "powerpc64"))
		))]
		{
			termios.c_cflag &= !(libc::CBAUD | libc::CIBAUD);
			termios.c_cflag |= libc::BOTHER;
			termios.c_cflag |= libc::BOTHER << libc::IBSHIFT;
			termios.c_ospeed = 57600;
			termios.c_ispeed = 57600;
			Ok(())
		}

		#[cfg(all(
			not(any(
				target_os = "dragonfly",
				target_os = "freebsd",
				target_os = "ios",
				target_os = "macos",
				target_os = "netbsd",
				target_os = "openbsd",
			)),
			not(all(
				any(target_os = "android", target_os = "linux"),
				not(any(target_arch = "powerpc", target_arch = "powerpc64"))
			)),
		))]
		unsafe {
			Self::check(libc::cfsetospeed(termios, libc::B57600))?;
			Self::check(libc::cfsetispeed(termios, libc::B57600))?;
			Ok(())
		}
	}

	/// Get the terminal interface flags for a particular file descriptor.
	///
	/// ## Errors
	///
	/// If we cannot call the appropriate api, or get an error back from the OS.
	fn get_termios_from_fd(fd: &File) -> IoResult<RawTermios> {
		#[cfg(all(
			any(target_os = "android", target_os = "linux"),
			not(any(target_arch = "powerpc", target_arch = "powerpc64"))
		))]
		unsafe {
			let mut termios = std::mem::zeroed();
			Self::check(libc::ioctl(
				fd.as_raw_fd(),
				libc::TCGETS2 as _,
				&mut termios,
			))?;
			Ok(termios)
		}

		#[cfg(not(all(
			any(target_os = "android", target_os = "linux"),
			not(any(target_arch = "powerpc", target_arch = "powerpc64"))
		)))]
		unsafe {
			let mut termios = std::mem::zeroed();
			Self::check(libc::tcgetattr(fd.as_raw_fd(), &mut termios))?;
			Ok(termios)
		}
	}

	/// Set the terminal interface flags on a file descriptor.
	///
	/// ## Errors
	///
	/// If we cannot call the appropriate api either `ioctl`, or `tcsetattr`
	/// on the underlying file descriptor, or we get an error back from the OS.
	fn set_termios_on_fd(fd: &File, termios: &RawTermios) -> IoResult<()> {
		#[cfg(all(
			any(target_os = "android", target_os = "linux"),
			not(any(target_arch = "powerpc", target_arch = "powerpc64"))
		))]
		unsafe {
			Self::check(libc::ioctl(fd.as_raw_fd(), libc::TCSETSW2 as _, termios))?;
			Ok(())
		}

		#[cfg(not(all(
			any(target_os = "android", target_os = "linux"),
			not(any(target_arch = "powerpc", target_arch = "powerpc64"))
		)))]
		unsafe {
			Self::check(libc::tcsetattr(fd.as_raw_fd(), libc::TCSADRAIN, termios))?;
			Ok(())
		}
	}

	/// Check a return code and turn it into an OS Error.
	///
	/// ## Errors
	///
	/// If the OS returns an error code that is -1.
	fn check(ret: i32) -> IoResult<i32> {
		if ret == -1 {
			Err(IoError::last_os_error())
		} else {
			Ok(ret)
		}
	}

	/// Check a return size and turn it into an OS Error.
	///
	/// ## Errors
	///
	/// If the OS returns an error code that is -1.
	fn check_isize(ret: isize) -> IoResult<isize> {
		if ret == -1 {
			Err(IoError::last_os_error())
		} else {
			Ok(ret)
		}
	}
}
