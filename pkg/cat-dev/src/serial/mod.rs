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

mod async_sys;
mod underlying;

pub use async_sys::*;
pub use underlying::*;
