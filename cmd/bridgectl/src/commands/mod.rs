//! A thin module wrapper that contains all the different files that each
//! handle one command.

mod add;
mod get;
mod help;
mod list;
mod remove;
mod set_default;

pub use add::*;
pub use get::*;
pub use help::*;
pub use list::*;
pub use remove::*;
pub use set_default::*;
