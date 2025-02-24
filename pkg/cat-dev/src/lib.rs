#![doc = include_str!("../README.md")]
#![allow(
	// I dislike this rule... We import things elsewhere, usually outside of
  // modules themselves.
	clippy::module_name_repetitions,
)]

pub mod errors;
pub mod fsemul;
pub mod mion;
#[cfg(any(feature = "clients", feature = "servers"))]
pub mod net;
#[cfg(feature = "serial")]
#[macro_use]
pub mod serial;

/// A Wii-U Title ID.
///
/// Wii-U Title IDs are normally 64 bits, but are generally treated by the
/// community as two seperate things. As the upper 32 bits represent the
/// general properties of the title. While the bottom 32 bits usually uniquely
/// identify the title itself.
///
/// TODO(mythra): find a better place to put this type.
pub type TitleID = (u32, u32);
