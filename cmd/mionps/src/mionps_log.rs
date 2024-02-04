//! Log statements to match the legacy output of mionps.

use std::net::Ipv4Addr;

use time::OffsetDateTime;

/// Create a logging hook to use when we're about to call connect with TCP.
pub fn create_session_logging_hook(verbose: bool) -> impl Fn(u128) + Clone + Send + 'static {
	if verbose {
		|timeout: u128| {
			log_verbose("Got TCP Session");
			log_verbose(&format!("Using asynchronous mode, timeout={timeout}"));
		}
	} else {
		|_timeout: u128| {}
	}
}

/// Create a logging hook to use when a connection is established.
pub fn create_connection_established_logging_hook(
	verbose: bool,
) -> impl Fn(Ipv4Addr) + Clone + Send + 'static {
	if verbose {
		|addr: Ipv4Addr| {
			log_verbose(&format!("Connection established to {addr}"));
		}
	} else {
		|_addr: Ipv4Addr| {}
	}
}

/// Create a logging hook to use when we have written bytes on a connection.
pub fn create_write_finished_logging_hook(
	verbose: bool,
) -> impl Fn(usize) + Clone + Send + 'static {
	if verbose {
		|expected_bytes_to_read: usize| {
			log_verbose("Write command send with READ request");
			log_verbose(&format!(
				"Configured next expected read of {expected_bytes_to_read} bytes"
			));
			log_verbose("MionPsConnCallback returning 0");
			log_verbose("write buffer flushed, p=0/00000000");
			log_verbose("MionPsConnCallback returning 0");
		}
	} else {
		|_expected_bytes_to_read: usize| {}
	}
}

/// Create a logging hook to use when we have read bytes on a connection.
pub fn create_read_finished_logging_hook(verbose: bool) -> impl Fn(usize) + Clone + Send + 'static {
	if verbose {
		|bytes_read: usize| {
			log_verbose(&format!("Good read of {bytes_read} bytes"));
		}
	} else {
		|_bytes_read: usize| {}
	}
}

/// Create a logging hook to use when we are about to set a new value.
pub fn create_set_new_value_logging_hook(
	verbose: bool,
) -> impl Fn(u8, u8, usize) + Clone + Send + 'static {
	if verbose {
		|old_value: u8, new_value: u8, index: usize| {
			log_verbose("About to update version in preparation for write");
			log_verbose("Done with version update, updated=FALSE");
			log_verbose(&format!(
				"Old Value: ({old_value}), Desired value: ({new_value}) at offset({index})"
			));
			log_verbose("About to issue command to set value(s)");
			log_verbose("setReq buffered in, setting next read size to 12");
			log_verbose("MionPsConnCallback returning 0");
		}
	} else {
		|_old_value: u8, _new_value: u8, _index: usize| {}
	}
}

pub fn create_write_set_finished_logging_hook(
	verbose: bool,
) -> impl Fn(usize) + Clone + Send + 'static {
	if verbose {
		|_read_buffer_size: usize| {
			log_verbose("write buffer flushed, p=0/00000000");
			log_verbose("MionPsConnCallback returning 0");
		}
	} else {
		|_read_buffer_size: usize| {}
	}
}

/// Exit if there was an error in the async function.
pub fn exit_async_error(ip: Ipv4Addr, verbose: bool) -> ! {
	// This is the error code used for a timeout, i don't really have a
	// better more generic error code here.
	if verbose {
		log_verbose("tcpSessionFn returned 5");
	}

	print!("mionps: ERROR: MIONTCPSession to ip {ip} returned error code 5\n\nERROR!");
	exit_with_verbose_message(17, verbose);
}

/// Do a verbose `WSACleanup` log message, and then exit.
pub fn exit_with_verbose_message(exit_code: i32, verbose: bool) -> ! {
	if verbose {
		log_verbose(&format!(
			"Done with WSACleanup, about to return {exit_code}"
		));
	}
	std::process::exit(exit_code);
}

/// Print an error message to the screen.
pub fn log_error(error_msg: &str) {
	print!("mionps: ERROR: {error_msg}\nERROR!");
}

/// Log a verbose message which includes a timestamp.
pub fn log_verbose(message: &str) {
	let current_time = OffsetDateTime::now_utc();
	println!(
		"mionps: VERBOSE: [{:02}/{:02}/{:04} {:02}:{:02}:{:02}.{:03}] {message}",
		u8::from(current_time.month()),
		current_time.day(),
		current_time.year(),
		current_time.hour(),
		current_time.minute(),
		current_time.second(),
		current_time.millisecond(),
	);
}
