//! Code for "PCFS", or the actual serving of a filesystem for the cat-dev.
//!
//! PCFS is the most common thing users are 'expecting' to interact with. It
//! provides a full filesystem to the cat-dev. The easiest way to think about
//! it might be like `FSEmul` provides access to raw block data, while `PCFS`
//! provides a filesystem ontop of that block data.
//!
//! It's main protocols implement things like `CreateDirectory`, `OpenFile`,
//! etc.

pub mod errors;
pub mod sata;
