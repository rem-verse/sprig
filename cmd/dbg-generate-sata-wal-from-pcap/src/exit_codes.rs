//! Just a list of all the exit codes in our process.

pub const NOT_YET_IMPLEMENTED: i32 = 1;
pub const LOGGING_HANDLER_INSTALL_FAILURE: i32 = 2;
pub const SHOULD_NEVER_HAPPEN_FAILURE: i32 = 3;
pub const FAILED_TO_WRITE_TO_DISK: i32 = 4;

pub const ARGV_PARSE_FAILURE: i32 = 10;
pub const ARGV_NO_COMMAND_SPECIFIED: i32 = 11;

pub const GENERATE_PCAP_DOES_NOT_EXIST: i32 = 20;
pub const GENERATE_CANT_CREATE_WAL: i32 = 21;
pub const GENERATE_CANT_OPEN_PCAP: i32 = 22;
pub const GENERATE_CANT_SPAWN_TSHARK: i32 = 23;
pub const GENERATE_NAGLE_FAILURE: i32 = 24;
