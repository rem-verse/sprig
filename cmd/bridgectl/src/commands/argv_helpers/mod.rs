//! Argument helpers for commands that take a lot of the exact same arguments.

mod bridge_conf;
mod bridge_scan;
mod bridge_target;
mod fsemul_conf;
mod serial;
mod shared_server;
mod strings;

pub use bridge_conf::*;
pub use bridge_scan::*;
pub use bridge_target::*;
pub use fsemul_conf::*;
pub use serial::*;
pub use shared_server::*;
pub use strings::*;
