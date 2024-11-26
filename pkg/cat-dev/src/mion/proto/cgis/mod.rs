//! Protocols specifically for talking with the "CGI" http pages of the MION
//! board.

mod control;
mod setup;
mod status;
mod update;

pub use control::*;
pub use setup::*;
pub use status::*;
pub use update::*;
