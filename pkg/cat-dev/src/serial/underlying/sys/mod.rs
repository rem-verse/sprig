//! Contains the per os raw implementations of a serial port.
//!
//! This implementations should never ideally be used directly, and are
//! effectively just thin wrappers around the OS APIs.

#[cfg(any(
	target_os = "dragonfly",
	target_os = "freebsd",
	target_os = "ios",
	target_os = "macos",
	target_os = "netbsd",
	target_os = "openbsd",
	target_os = "linux",
	target_os = "android",
	target_os = "illumos",
	target_os = "solaris",
))]
mod unix;
#[cfg(any(
	target_os = "dragonfly",
	target_os = "freebsd",
	target_os = "ios",
	target_os = "macos",
	target_os = "netbsd",
	target_os = "openbsd",
	target_os = "linux",
	target_os = "android",
	target_os = "illumos",
	target_os = "solaris",
))]
pub use unix::*;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;

/// The default timeout in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u32 = 3000;
