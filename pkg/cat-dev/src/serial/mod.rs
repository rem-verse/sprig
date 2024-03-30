//! Tools for interacting with the serial port of a cat-dev.
//!
//! This interface was originally a fork of:
//! <https://github.com/de-vri-es/serial2-tokio-rs> at commit:
//! `65ff229f65c27c57e261f94dc6cc9a761cce9b21`.
//! And
//! <https://github.com/de-vri-es/serial2-rs/> at commit:
//! `dc1333ce8f205e77cb2a89d2ed52463ff56cdc04`
//!
//! You can see the dual apache/bsd licenses for them at:
//! <https://raw.githubusercontent.com/de-vri-es/serial2-tokio-rs/65ff229f65c27c57e261f94dc6cc9a761cce9b21/LICENSE-APACHE>
//! <https://raw.githubusercontent.com/de-vri-es/serial2-tokio-rs/65ff229f65c27c57e261f94dc6cc9a761cce9b21/LICENSE-BSD>
//!
//! This fork has had minor changes to it, mostly updating to dependencies
//! like `windows`, over `winapi` and other logging integrations that we want
//! the overarching library to have, etc.
//!
//! We also contain a slightly-modified copy of the [`tokio::io::Lines`]
//! structure from [`tokio`], but one that reads the newlines of the
//! serial port `\r`, as opposed to the normal `.lines()` method
//! which uses `\n`.

mod async_sys;
mod underlying;

pub use async_sys::*;
pub use underlying::*;

use pin_project_lite::pin_project;
use std::{
	fmt::{Debug, Formatter, Result as FmtResult},
	future::Future,
	pin::Pin,
	string::FromUtf8Error,
	task::{Context, Poll},
};
use tokio::io::{AsyncBufRead, Error as IoError, ErrorKind as IoErrorKind, Result as IoResult};

macro_rules! ready {
	($e:expr $(,)?) => {
		match $e {
			std::task::Poll::Ready(t) => t,
			std::task::Poll::Pending => return std::task::Poll::Pending,
		}
	};
}

pin_project! {
	/// Reads serial lines from an [`AsyncBufRead`].
	///
	/// This tries to act a lot like the structure returned from `.lines()` on a
	/// normal async buf reader. Except instead of splitting on `\n`, we use
	/// `\r`.
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
	/// Grab the next line from the serial line reader.
	///
	/// ## Errors
	///
	/// If we get an [`IoError`] back from the underlying reader we're reading
	/// from.
	pub async fn next_line(&mut self) -> IoResult<Option<String>> {
		poll_fn(|cx| Pin::new(&mut *self).poll_next_line(cx)).await
	}
}

impl<ReaderTy> SerialLines<ReaderTy>
where
	ReaderTy: AsyncBufRead,
{
	/// Create a new Serial Lines reader.
	pub fn new(reader: ReaderTy) -> Self {
		SerialLines {
			reader,
			buf: String::new(),
			bytes: Vec::new(),
			read: 0,
		}
	}

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
	) -> Poll<IoResult<Option<String>>> {
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
) -> Poll<IoResult<usize>> {
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
) -> Poll<IoResult<usize>> {
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
struct PollFn<F> {
	f: F,
}

/// Creates a new future wrapping around a function returning [`Poll`].
fn poll_fn<T, F>(f: F) -> PollFn<F>
where
	F: FnMut(&mut Context<'_>) -> Poll<T>,
{
	PollFn { f }
}

impl<F> Debug for PollFn<F> {
	fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
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
	io_res: IoResult<usize>,
	utf8_res: Result<String, FromUtf8Error>,
	read: usize,
	output: &mut String,
	truncate_on_io_error: bool,
) -> Poll<IoResult<usize>> {
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
			Poll::Ready(Err(IoError::new(
				IoErrorKind::InvalidData,
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
