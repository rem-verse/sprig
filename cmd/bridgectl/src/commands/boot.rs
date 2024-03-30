//! Perform booting of a cat-dev bridge.
//!
//! This is by far the most complex command, and has to orchestrate the most
//! amount of things in order for things to all go off smoothly. There are
//! lots of potential different paths here, but let's sum up the biggest
//! differences at all:
//!
//! PCFS vs Non-PCFS. Most CAT-DEVs are setup to use PCFS as their boot mode,
//! this means they need multiple servers/connections running in order to
//! emulate the disk drive, file system, and more. This is also why most people
//! boot up a cat-dev, see a black screen and assume it's broken. Because the
//! special PC software isn't running to set it all up.
//!
//! Not to mention it requires a filesystem to actually exist on the computer, so
//! you need a fully configured SDK setup that can properly serve the filesystem
//! to the device... yikes! (It's also why most files weren't leaked from
//! CAT-DEV internal harddrives, many games just never lived there).
//!
//! The other option is a "non-pcfs" boot, which can be forced even if the
//! device normally requires a pc server to be running. This isn't techincally
//! fully supported, but is possible to do with the `boot-without-pcfs` option,
//! or if the MION is configured to not boot without it.
//!
//! Secondly we also want to tail all the serial port logs happening during the
//! boot process, and keep those serial port logs streaming until a user hits
//! CTRL-C (if we've got a serial port configured). This is because boot logs are
//! streamed out over a Serial connection, and we want to be sure to catch
//! them.
//!
//! Finally, there is one corner case of "taking over" a MION that is already
//! booted from another machine. I'm not sure how often this was used, and I
//! don't really recommend it, but it is possible to take ownership of another
//! host.

use crate::{
	commands::argv_helpers::{
		coalesce_serial_ports, get_targeted_bridge_ip, get_targeted_bridge_name,
		spawn_serial_log_task,
	},
	exit_codes::{BOOT_CGI_FAILURE, BOOT_NOT_READY_TO_BOOT, NOT_YET_IMPLEMENTED},
	SHOULD_LOG_JSON,
};
use cat_dev::mion::cgis::{get_info, power_on_v2};
use miette::miette;
use std::{net::Ipv4Addr, path::PathBuf};
use tokio::task::JoinHandle;
use tracing::{error, field::valuable, info, warn};

pub async fn handle_boot(
	no_pcfs: bool,
	serial_port_args: (Option<PathBuf>, Option<PathBuf>),
	take_ownership: bool,
) {
	let bridge_ip = get_targeted_bridge_ip().await;
	let bridge_name = get_targeted_bridge_name().await;

	let mut serial_task_handle = None;
	if let Some((serial_port, serial_path)) =
		coalesce_serial_ports(serial_port_args.0.as_ref(), serial_port_args.1.as_ref())
	{
		serial_task_handle = Some(spawn_serial_log_task(serial_port, serial_path));
	}
	let needs_pcfs =
		validate_bridge_ready_for_booting(take_ownership, bridge_ip, &bridge_name).await;

	// TODO(mythra): can take ownership? through set param?

	if no_pcfs || !needs_pcfs {
		boot_without_pcfs(needs_pcfs, bridge_ip, serial_task_handle).await;
		return;
	}

	error!("Sorry not yet implemented!");
	std::process::exit(NOT_YET_IMPLEMENTED);
}

async fn boot_without_pcfs(
	needs_pcfs: bool,
	bridge_ip: Ipv4Addr,
	serial_handle: Option<JoinHandle<()>>,
) {
	if needs_pcfs {
		if SHOULD_LOG_JSON() {
			warn!(
				id = "bridgectl::boot::override_pcfs",
				reason = "manually_requested",
				"overriding PCFS boot mode, the cat-dev will still have an error light but should at least boot to SystemConfigTool",
			);
		} else {
			warn!(
				reason = "manually_requested",
				"Overriding PCFS boot mode, the cat-dev will still have an error light but should at least boot to SystemConfigTool",
			);
		}
	}

	match power_on_v2(bridge_ip, None, None, false).await {
		Ok(result_code) => {
			if result_code {
				info!("Successfully powered on MION!");
				if let Some(hndl) = serial_handle {
					_ = hndl.await;
				}
			} else {
				error!("Failed to boot cat-dev bridge! Please reach out for support!");
				std::process::exit(BOOT_CGI_FAILURE);
			}
		}
		Err(cause) => {
			if SHOULD_LOG_JSON() {
				error!(
					id = "bridgectl::boot::failed_to_boot_device",
					bridge.ip = %bridge_ip,
					?cause,
					suggestions = valuable(&[
						"Please file an issue, and reach out!"
					]),
				);
			} else {
				error!(
					"\n{:?}",
					miette!(
						help = "PLEASE PLEASE PLEASE FILE AN ISSUE!",
						"Failure to perform non PCFS boot!!! THIS IS STILL EARLY !!! PLEASE FILE AN ISSUE!\n {cause:?}",
					),
				);
			}

			std::process::exit(BOOT_CGI_FAILURE);
		}
	}
}

/// Validate a bridge is ready to be booted, and return if it needs PCFS.
#[must_use]
async fn validate_bridge_ready_for_booting(
	will_take_over: bool,
	bridge_ip: Ipv4Addr,
	bridge_name: &str,
) -> bool {
	let information_request = match get_info(bridge_ip, bridge_name).await {
		Ok(map) => map,
		Err(cause) => {
			if SHOULD_LOG_JSON() {
				error!(
					id = "bridgectl::boot::get_bridge_info",
					?cause,
					"failed to query bridge information to make sure it was ready for booting",
				);
			} else {
				error!(
					"\n{:?}",
					miette!(
						help = format!("You can ensure the bridge is ready to be viewed at: <http://{bridge_ip}/menu.cgi>"),
						"Failed to query bridge information to make sure it was ready for booting, please make sure the device is on, and has it's webpages running."
					).wrap_err(cause),
				);
			}

			std::process::exit(BOOT_CGI_FAILURE);
		}
	};

	if information_request
		.get("RESULT")
		.map(String::as_str)
		.unwrap_or_default()
		!= "OK"
	{
		if SHOULD_LOG_JSON() {
			error!(
				id = "bridgectl::boot::check_info_result",
				response_fields = ?information_request,
				"Result was not okay for receiving information",
			);
		} else {
			error!(
				"\n{:?}",
				miette!(
					help = format!(
						"debugging note, all results from the cat-dev: {information_request:?}"
					),
					"Fetching information was not successful",
				),
			);
		}

		std::process::exit(BOOT_NOT_READY_TO_BOOT);
	}

	if information_request
		.get("curhost")
		.map(String::as_str)
		.unwrap_or_default()
		!= "0.0.0.0"
	{
		if SHOULD_LOG_JSON() {
			warn!(
				id = "bridgectl::boot::owned_by_other_host",
				response_fields = ?information_request,
				"MION is currently being managed by another host!!",
			);
		} else {
			warn!(
				"\n{:?}",
				miette!(
					help = format!(
						"debugging note, all results from the cat-dev: {information_request:?}"
					),
					"MION is currently being managed by another host!!",
				),
			);
		}

		if !will_take_over {
			error!(
				"Did not specify `--take-ownership`, exiting because MION is owned by another..."
			);
			std::process::exit(BOOT_NOT_READY_TO_BOOT);
		}
	}

	let boot_mode = information_request
		.get("bootmode")
		.map(String::as_str)
		.unwrap_or_default()
		.parse::<u8>()
		.unwrap_or(u8::MAX);
	boot_mode == 2
}
