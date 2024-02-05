//! MION is the Sata Device present on the CAT-DEV Bridge.
//!
//! In general MION's are pretty much the "host bridge" most of the host bridge
//! software _actually_ talks too. `hostdisplayversion` calls it the
//! "Bridge Type", but I don't think we  know of any other bridge types.
//!
//! In general if you're trying to look for things relating to the bridge as a
//! whole, you're _probably_ really actually talking to the MION.

pub mod discovery;
pub mod parameter;
pub mod proto;
