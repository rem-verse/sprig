//! A thin module wrapper that contains all the different files that each
//! handle one command.

mod generate;
mod help;
mod padlog;

pub use generate::*;
pub use help::*;
pub use padlog::*;
