#![doc = include_str!("../README.md")]
#![allow(
	// I dislike this rule... We import things elsewhere, usually outside of
  // modules themselves.
	clippy::module_name_repetitions,
)]

pub mod errors;
pub mod fsemul;
pub mod mion;
#[macro_use]
pub mod serial;
