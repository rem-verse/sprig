//! This module is a wrapper for all the underlying protocol types used for
//! interacting with MIONs.
//!
//! In general as a user you probably don't need to interact with this directly
//! except for maybe importing a few types that APIs return. In general you
//! probably want to use the functions available in the other modules under
//! [`crate::mion`] in order to interact with the MIONs in a safe way.
//!
//! Each of these roughly correlate to one MION service, e.g.:
//!
//! - port 7974 UDP is the "control" port, which can be used for discovery. So
//!   for communicating on that port you access:
//!   [`crate::mion::proto::control`].
//!   *note: some tools using session manager improperly use the ATAPI port
//!   which while normally being shared just for the TCP side, can in theory
//!   be configured differently.*
//!
//! - port 7978 TCP on the other hand is used by `mionps` to look up
//!   parameters, so we call it the "parameter" port, so you can access types
//!   for communicating on that port under: [`crate::mion::proto::parameter`].
//!   The official tools don't have a way of specifying this port, but it is
//!   actually configurable in `http://<mionip>/setup.cgi`. Specifically you
//!   can change it under "Parameter Space".

pub mod cgis;
pub mod control;
pub mod parameter;

/// The port the MION uses for 'control' commands.
pub const DEFAULT_MION_CONTROL_PORT: u16 = 7974;
/// The port the MION uses for parameter commands.
pub const DEFAULT_MION_PARAMETER_PORT: u16 = 7978;

/// The amount of seconds we'll wait for a MION Control board to respond to a
/// ping.
pub const MION_ANNOUNCE_TIMEOUT_SECONDS: u64 = 10;
/// MION timeouts for sending packets directly to the MION, on the parameter
/// port.
pub const MION_PARAMETER_TIMEOUT_SECONDS: u64 = 5;
