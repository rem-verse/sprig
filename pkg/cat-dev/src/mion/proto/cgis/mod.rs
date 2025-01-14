//! Protocols specifically for talking with the "CGI" http pages of the MION
//! board.

mod control;
mod errors;
mod setup;
mod status;
mod update;

pub use control::*;
pub use errors::*;
pub use setup::*;
pub use status::*;
pub use update::*;
