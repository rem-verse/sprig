//! Just a list of all the exit codes in our process.

pub const LOGGING_HANDLER_INSTALL_FAILURE: i32 = 1;
pub const ARGUMENT_PARSING_FAILURE: i32 = 2;
pub const NO_ARGUMENT_SPECIFIED_FAILURE: i32 = 3;
pub const SHOULD_NEVER_HAPPEN_FAILURE: i32 = 4;
pub const CANT_FIND_BRIDGE_STATE_PATH: i32 = 5;
pub const CANT_LOAD_BRIDGE_STATE: i32 = 6;
pub const CONFLICTING_ARGUMENTS_FOR_ADD: i32 = 7;
pub const NO_SPECIFIER_FOR_ADD: i32 = 8;
pub const ADD_COULD_NOT_SEARCH: i32 = 9;
pub const ADD_COULD_NOT_FIND: i32 = 10;
pub const ADD_COULD_NOT_UPSERT: i32 = 11;
pub const ADD_COULD_NOT_SAVE_TO_DISK: i32 = 12;
pub const LIST_COULD_NOT_SEARCH: i32 = 13;
pub const GET_DEFAULT_WITH_FILTERS: i32 = 14;
pub const GET_DEFAULT_CONFLICTING_FILTERS: i32 = 15;
pub const GET_FAILED_TO_SEARCH_FOR_DEVICE: i32 = 16;
pub const GET_FAILED_TO_FIND_SPECIFIC_DEVICE: i32 = 17;
pub const REMOVE_CONFLICTING_ARGUMENTS: i32 = 18;
pub const REMOVE_NO_ARGUMENTS: i32 = 19;
pub const REMOVE_BRIDGE_DOESNT_EXIST: i32 = 20;
pub const REMOVE_COULD_NOT_SAVE_TO_DISK: i32 = 21;
pub const SET_DEFAULT_CONFLICTING_ARGUMENTS: i32 = 22;
pub const SET_DEFAULT_NO_ARGUMENTS: i32 = 23;
pub const SET_DEFAULT_BRIDGE_DOESNT_EXIST: i32 = 24;
pub const SET_DEFAULT_COULD_NOT_SAVE_TO_DISK: i32 = 25;
pub const GET_PARAMS_NO_AVAILABLE_BRIDGE: i32 = 26;
pub const ARGV_COULD_NOT_GET_DEFAULT_BRIDGE: i32 = 27;
pub const GET_PARAMS_NO_PARAMETERS_SPECIFIED: i32 = 28;
pub const GET_PARAMS_FAILED_TO_GET_PARAMS: i32 = 29;
pub const SET_PARAMS_NO_AVAILABLE_BRIDGE: i32 = 30;
pub const SET_PARAMS_NO_PARAMETERS_SPECIFIED: i32 = 31;
pub const SET_PARAMS_INVALID_PARAMETER_SET_STRING: i32 = 32;
pub const SET_PARAMS_INVALID_PARAMETER_VALUE: i32 = 33;
pub const SET_PARAMS_FAILED_TO_SET_PARAMS: i32 = 34;
