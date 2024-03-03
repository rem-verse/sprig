//! CGI's that are available to interact with the MION on.
//!
//! These various CGI web pages you can interact with normally on the web.

/// HTTP Basic authorization header.
///
/// This gets passed as the header:
/// `Authorization: Basic bWlvbjovTXVsdGlfSS9PX05ldHdvcmsv`
///
/// Given this is http basic auth, you can decode this string as:
/// `mion:/Multi_I/O_Network/`
///
/// Which means the username is: `mion`, and the password is:
/// `/Multi_I/O_Network/`.
const AUTHZ_HEADER: &str = "bWlvbjovTXVsdGlfSS9PX05ldHdvcmsv";

mod control;
mod signal_get;

pub use control::*;
pub use signal_get::*;
