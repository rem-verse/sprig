//! These "handler" types, are a way of turning raw rust functions
//! into a [`tower::Service`].
//!
//! This way they can be plugged into TCP servers to handle packets
//! without requiring the function to be decorated.

mod on_stream_begin_handlers;
mod on_stream_end_handlers;

pub use on_stream_begin_handlers::*;
pub use on_stream_end_handlers::*;
