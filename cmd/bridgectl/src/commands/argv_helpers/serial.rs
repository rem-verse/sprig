use crate::{
	exit_codes::{
		CONFLICTING_SERIAL_PORT_ARGS, SERIAL_PORT_CONNECTION_FAILURE, SHOULD_NEVER_HAPPEN_FAILURE,
	},
	knobs::env::BRIDGECTL_SERIAL_PORT,
	utils::add_context_to,
};
use cat_dev::serial::AsyncSerialPort;
use miette::miette;
use pin_project_lite::pin_project;
use std::{
	future::Future,
	path::PathBuf,
	pin::Pin,
	string::FromUtf8Error,
	task::{Context, Poll},
};
use tokio::{
	io::{AsyncBufRead, BufReader},
	signal::ctrl_c as ctrl_c_signal,
	task::{Builder as TaskBuilder, JoinHandle},
};
use tracing::{debug, error, field::valuable, info, warn};

macro_rules! ready {
	($e:expr $(,)?) => {
		match $e {
			std::task::Poll::Ready(t) => t,
			std::task::Poll::Pending => return std::task::Poll::Pending,
		}
	};
}

/// Coalesce all serial port arguments into a serial port.
///
/// ## Panics
///
/// - If conflicting arguments (conflicting arg + env do not panic) are specified
///   for a serial port.
/// - If we cannot open a handle/descriptor to the associated serial device.
pub fn coalesce_serial_ports(
	use_json: bool,
	serial_port_flag: Option<&PathBuf>,
	serial_port_positional: Option<&PathBuf>,
) -> Option<(AsyncSerialPort, PathBuf)> {
	let arg_to_take = if serial_port_flag.is_some() && serial_port_positional.is_some() {
		if use_json {
			error!(
				id = "bridgectl::argv::conflicting_serial_port_args",
				flags.serial_port = ?serial_port_flag,
				args.serial_port = ?serial_port_positional,
				suggestions = valuable(&[
					"You only need to specify a serial port in one way, either through an argument, or a flag.",
					"There is no such thing as multiple serial ports for the cat-dev.",
				]),
			);
		} else {
			error!(
			"\n{:?}",
			add_context_to(
				miette!("Positional argument conflicts with flag arguments!"),
				[
					miette!("You only need to specify a serial port in one way, either through an argument, or a flag."),
					miette!(
						help = format!(
							"Serial Port Flag: `{:?}` / Positional Argument: `{:?}`",
							serial_port_flag,
							serial_port_positional,
						),
						"A CAT-DEV does not support multiple serial ports at the same time.",
					),
				].into_iter(),
			),
		);
		}

		std::process::exit(CONFLICTING_SERIAL_PORT_ARGS);
	} else if let Some(flag) = serial_port_flag {
		flag
	} else if let Some(pos) = serial_port_positional {
		pos
	} else {
		BRIDGECTL_SERIAL_PORT.as_ref()?
	};

	let port = match AsyncSerialPort::new(arg_to_take) {
		Ok(port) => port,
		Err(cause) => {
			if use_json {
				error!(
					id = "bridgectl::argv::serial_connection_failure",
					?cause,
					help = "Please file an issue if it's not clear with your serial device.",
					port = %arg_to_take.display(),
					"failed to connect to serial device specified"
				);
			} else {
				error!(
					"\n{:?}",
					add_context_to(
						miette!("{cause:?}"),
						[
							miette!("Failed to connect to specified serial device."),
							miette!(
								help = format!("Specified serial device is: {}", arg_to_take.display()),
								"Please file an issue if it's not clear why your OS is giving us an error.",
							),
						]
						.into_iter(),
					),
				);
			}

			std::process::exit(SERIAL_PORT_CONNECTION_FAILURE);
		}
	};

	Some((port, arg_to_take.clone()))
}

/// Spawn a task that reads from a serial port over, and over again.
#[allow(clippy::blocks_in_conditions)]
pub fn spawn_serial_log_task(
	use_json: bool,
	port: AsyncSerialPort,
	port_path: PathBuf,
) -> JoinHandle<()> {
	let handle = match TaskBuilder::new()
		.name("bridgectl::serial_log::watcher")
		.spawn(async move {
			let mut reader = SerialLines {
				reader: BufReader::new(port),
				buf: String::new(),
				bytes: Vec::new(),
				read: 0,
			};

			loop {
				tokio::select! {
					res = reader.next_line() => {
						match res {
							Ok(Some(line)) => if use_json {
								info!(
									id = "bridgectl::serial_log::watcher::line",
									port = %port_path.display(),
									%line,
									"received log line from serial port",
								);
							} else {
								info!(
									port = %port_path.display(),
									line,
								);
							}
							Ok(None) => {
								if use_json {
									debug!(
										id = "bridgectl::serial_log::watcher::graceful_shutdown",
										shutdown_reason = "empty-receive",
										"shutting down gracefully"
									);
								} else {
									debug!(
										shutdown_reason = "empty-receive",
										"shutting down serial log watcher gracefully..."
									);
								}

								break;
							}
							Err(cause) => {
								if use_json {
									warn!(
										id = "bridgectl::serial_log::watcher::failure",
										?cause,
										"could not receive lines from this serial port."
									);
								} else {
									warn!(?cause, "serial port gave us an error trying to read from it.");
								}

								break;
							}
						}
					}
					_ = ctrl_c_signal() => {
						if use_json {
							debug!(
								id = "bridgectl::serial_log::watcher::graceful_shutdown",
								shutdown_reason = "ctrl-c",
								"shutting down gracefully"
							);
						} else {
							debug!(
								shutdown_reason = "ctrl-c",
								"shutting down serial log watcher gracefully..."
							);
						}

						break;
					}
				}
			}
		}) {
		Ok(port) => port,
		Err(cause) => {
			if use_json {
				warn!(
					id = "bridgectl::serial::watcher_spawn_failure",
					?cause,
					"failed to spawn task to watch serial logs; internal",
				);
			} else {
				warn!(
					?cause,
					"internal error: failed to spawn task to watch for serial logs, serial logs will not be watched for.",
				);
			}

			std::process::exit(SHOULD_NEVER_HAPPEN_FAILURE);
		}
	};

	handle
}

pin_project! {
	/// Reads serial lines from an [`AsyncBufRead`].
	///
	/// A `Lines` can be turned into a `Stream` with [`LinesStream`].
	///
	/// [`AsyncBufRead`]: tokio::io::AsyncBufRead
	/// [`LinesStream`]: https://docs.rs/tokio-stream/0.1/tokio_stream/wrappers/struct.LinesStream.html
	#[derive(Debug)]
	#[must_use = "streams do nothing unless polled"]
	pub struct SerialLines<ReaderTy> {
		#[pin]
		reader: ReaderTy,
		buf: String,
		bytes: Vec<u8>,
		read: usize,
	}
}
impl<ReaderTy> SerialLines<ReaderTy>
where
	ReaderTy: AsyncBufRead + Unpin,
{
	pub async fn next_line(&mut self) -> tokio::io::Result<Option<String>> {
		poll_fn(|cx| Pin::new(&mut *self).poll_next_line(cx)).await
	}
}
impl<ReaderTy> SerialLines<ReaderTy>
where
	ReaderTy: AsyncBufRead,
{
	/// Polls for the next line in the stream.
	///
	/// This method returns:
	///
	///  * `Poll::Pending` if the next line is not yet available.
	///  * `Poll::Ready(Ok(Some(line)))` if the next line is available.
	///  * `Poll::Ready(Ok(None))` if there are no more lines in this stream.
	///  * `Poll::Ready(Err(err))` if an IO error occurred while reading the next line.
	///
	/// When the method returns `Poll::Pending`, the `Waker` in the provided
	/// `Context` is scheduled to receive a wakeup when more bytes become
	/// available on the underlying IO resource.  Note that on multiple calls to
	/// `poll_next_line`, only the `Waker` from the `Context` passed to the most
	/// recent call is scheduled to receive a wakeup.
	pub fn poll_next_line(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
	) -> Poll<tokio::io::Result<Option<String>>> {
		let me = self.project();

		let n = ready!(read_line_internal(me.reader, cx, me.buf, me.bytes, me.read))?;
		debug_assert_eq!(*me.read, 0);

		if n == 0 && me.buf.is_empty() {
			return Poll::Ready(Ok(None));
		}

		if me.buf.ends_with('\r') {
			me.buf.pop();
		}

		Poll::Ready(Ok(Some(std::mem::take(me.buf))))
	}
}
fn read_line_internal<ReaderTy: AsyncBufRead + ?Sized>(
	reader: Pin<&mut ReaderTy>,
	cx: &mut Context<'_>,
	output: &mut String,
	buf: &mut Vec<u8>,
	read: &mut usize,
) -> Poll<tokio::io::Result<usize>> {
	let io_res = ready!(read_until_internal(reader, cx, b'\r', buf, read));
	let utf8_res = String::from_utf8(std::mem::take(buf));
	// At this point both buf and output are empty. The allocation is in utf8_res.
	debug_assert!(buf.is_empty());
	debug_assert!(output.is_empty());
	finish_string_read(io_res, utf8_res, *read, output, false)
}
fn read_until_internal<ReaderTy: AsyncBufRead + ?Sized>(
	mut reader: Pin<&mut ReaderTy>,
	cx: &mut Context<'_>,
	delimiter: u8,
	buf: &mut Vec<u8>,
	read: &mut usize,
) -> Poll<tokio::io::Result<usize>> {
	loop {
		let (done, used) = {
			let available = ready!(reader.as_mut().poll_fill_buf(cx))?;
			if let Some(i) = memchr(delimiter, available) {
				buf.extend_from_slice(&available[..=i]);
				(true, i + 1)
			} else {
				buf.extend_from_slice(available);
				(false, available.len())
			}
		};
		reader.as_mut().consume(used);
		*read += used;
		if done || used == 0 {
			return Poll::Ready(Ok(std::mem::replace(read, 0)));
		}
	}
}

#[cfg(not(unix))]
fn memchr(needle: u8, haystack: &[u8]) -> Option<usize> {
	haystack.iter().position(|val| needle == *val)
}

#[cfg(unix)]
#[allow(clippy::cast_lossless)]
fn memchr(needle: u8, haystack: &[u8]) -> Option<usize> {
	let start = haystack.as_ptr();

	// SAFETY: `start` is valid for `haystack.len()` bytes.
	let ptr = unsafe { libc::memchr(start.cast(), needle as _, haystack.len()) };

	if ptr.is_null() {
		None
	} else {
		Some(ptr as usize - start as usize)
	}
}

// This struct is intentionally `!Unpin` when `F` is `!Unpin`. This is to
// mitigate the issue where rust puts noalias on mutable references to the
// `PollFn` type if it is `Unpin`. If the closure has ownership of a future,
// then this "leaks" and the future is affected by noalias too, which we don't
// want.
//
// See this thread for more information:
// <https://internals.rust-lang.org/t/surprising-soundness-trouble-around-pollfn/17484>

/// Future for the [`poll_fn`] function.
pub struct PollFn<F> {
	f: F,
}
/// Creates a new future wrapping around a function returning [`Poll`].
pub fn poll_fn<T, F>(f: F) -> PollFn<F>
where
	F: FnMut(&mut Context<'_>) -> Poll<T>,
{
	PollFn { f }
}
impl<F> std::fmt::Debug for PollFn<F> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("PollFn").finish()
	}
}
impl<T, F> Future for PollFn<F>
where
	F: FnMut(&mut Context<'_>) -> Poll<T>,
{
	type Output = T;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
		// Safety: We never construct a `Pin<&mut F>` anywhere, so accessing `f`
		// mutably in an unpinned way is sound.
		//
		// This use of unsafe cannot be replaced with the pin-project macro
		// because:
		//  * If we put `#[pin]` on the field, then it gives us a `Pin<&mut F>`,
		//    which we can't use to call the closure.
		//  * If we don't put `#[pin]` on the field, then it makes `PollFn` be
		//    unconditionally `Unpin`, which we also don't want.
		let me = unsafe { Pin::into_inner_unchecked(self) };
		(me.f)(cx)
	}
}

fn put_back_original_data(output: &mut String, mut vector: Vec<u8>, num_bytes_read: usize) {
	let original_len = vector.len() - num_bytes_read;
	vector.truncate(original_len);
	*output = String::from_utf8(vector).expect("The original data must be valid utf-8.");
}

/// This handles the various failure cases and puts the string back into `output`.
///
/// The `truncate_on_io_error` `bool` is necessary because `read_to_string` and `read_line`
/// disagree on what should happen when an IO error occurs.
fn finish_string_read(
	io_res: tokio::io::Result<usize>,
	utf8_res: Result<String, FromUtf8Error>,
	read: usize,
	output: &mut String,
	truncate_on_io_error: bool,
) -> Poll<tokio::io::Result<usize>> {
	match (io_res, utf8_res) {
		(Ok(num_bytes), Ok(string)) => {
			debug_assert_eq!(read, 0);
			*output = string;
			Poll::Ready(Ok(num_bytes))
		}
		(Err(io_err), Ok(string)) => {
			*output = string;
			if truncate_on_io_error {
				let original_len = output.len() - read;
				output.truncate(original_len);
			}
			Poll::Ready(Err(io_err))
		}
		(Ok(num_bytes), Err(utf8_err)) => {
			debug_assert_eq!(read, 0);
			put_back_original_data(output, utf8_err.into_bytes(), num_bytes);
			Poll::Ready(Err(tokio::io::Error::new(
				tokio::io::ErrorKind::InvalidData,
				"stream did not contain valid UTF-8",
			)))
		}
		(Err(io_err), Err(utf8_err)) => {
			put_back_original_data(output, utf8_err.into_bytes(), read);
			Poll::Ready(Err(io_err))
		}
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[test]
	fn memchr_test() {
		let haystack = b"123abc456\0\xffabc\n";
		assert_eq!(memchr(b'1', haystack), Some(0));
		assert_eq!(memchr(b'2', haystack), Some(1));
		assert_eq!(memchr(b'3', haystack), Some(2));
		assert_eq!(memchr(b'4', haystack), Some(6));
		assert_eq!(memchr(b'5', haystack), Some(7));
		assert_eq!(memchr(b'6', haystack), Some(8));
		assert_eq!(memchr(b'7', haystack), None);
		assert_eq!(memchr(b'a', haystack), Some(3));
		assert_eq!(memchr(b'b', haystack), Some(4));
		assert_eq!(memchr(b'c', haystack), Some(5));
		assert_eq!(memchr(b'd', haystack), None);
		assert_eq!(memchr(b'A', haystack), None);
		assert_eq!(memchr(0, haystack), Some(9));
		assert_eq!(memchr(0xff, haystack), Some(10));
		assert_eq!(memchr(0xfe, haystack), None);
		assert_eq!(memchr(1, haystack), None);
		assert_eq!(memchr(b'\n', haystack), Some(14));
		assert_eq!(memchr(b'\r', haystack), None);
	}

	#[test]
	fn memchr_all() {
		let mut arr = Vec::new();
		for b in 0..=255 {
			arr.push(b);
		}
		for b in 0..=255 {
			assert_eq!(memchr(b, &arr), Some(b as usize));
		}
		arr.reverse();
		for b in 0..=255 {
			assert_eq!(memchr(b, &arr), Some(255 - b as usize));
		}
	}

	#[test]
	fn memchr_empty() {
		for b in 0..=255 {
			assert_eq!(memchr(b, b""), None);
		}
	}
}
