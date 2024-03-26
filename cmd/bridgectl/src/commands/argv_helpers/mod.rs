//! Argument helpers for commands that take a lot of the exact same arguments.

mod bridge;
#[macro_use]
mod serial;
mod strings;

pub use bridge::*;
pub use serial::{coalesce_serial_ports, spawn_serial_log_task};
pub use strings::*;
