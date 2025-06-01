//! Implements things that can decorate a Request, and all implement
//! [`crate::net::models::FromRequest`].

mod body;
mod extension;
mod state;

pub use body::*;
pub use extension::*;
pub use state::*;
