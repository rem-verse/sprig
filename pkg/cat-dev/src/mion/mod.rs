//! MION is the Sata Device present on the CAT-DEV Bridge.
//!
//! In general MION's are pretty much the "host bridge" most of the host bridge
//! software _actually_ talks too. `hostdisplayversion` calls it the
//! "Bridge Type", but I don't think we  know of any other bridge types.
//!
//! In general if you're trying to look for things relating to the bridge as a
//! whole, you're _probably_ really actually talking to the MION.

pub mod discovery;
pub mod proto;

/// The Port the MION uses for 'control' commands.
pub const MION_CONTROL_PORT: u16 = 7974;
/// The amount of seconds we'll wait for a MION Control board to respond to a
/// ping.
pub const MION_ANNOUNCE_TIMEOUT_SECONDS: u64 = 10;
