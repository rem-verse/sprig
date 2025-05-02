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
//!
//! ## Mion FW Differences
//!
//! The booting process is pretty heavily impacted by the version of the Mion
//! FW you are running. For example only the latest firmware we have dumped
//! `0.00.14.77` has support for "taking over" a MION. Any firmware before that
//! may not have the capability to do so (we currently only have versions the
//! following versions from the important range:
//! `0.00.14.70` - no support, `0.00.14.77` - support, `0.00.14.80` - support).
//!
//! The raw Power ON APIs are also not as configurable when running versions
//! pre `0.00.14.77`, where `power_on_v2` exists.

mod atapi;
mod sata;
mod sdio;
mod utils;

use crate::{
	SHOULD_LOG_JSON,
	commands::{
		argv_helpers::{
			coalesce_serial_ports, get_atapi_port, get_host_bind_address, get_pcfs_sata_port,
			get_sdio_control_port, get_sdio_printf_port, get_targeted_bridge_ip,
			get_targeted_bridge_mac, get_targeted_bridge_name, lease_fsemul_config_optionally,
			lease_host_file_system,
		},
		boot::{
			atapi::serve_atapi,
			sata::serve_sata,
			sdio::serve_sdio,
			utils::{
				is_modern_bridge, turn_down_for_disc, validate_bridge_ready_for_booting,
				wrap_power_on,
			},
		},
	},
	exit_codes::BOOT_CGI_FAILURE,
	knobs::{
		cli::{FSEmulConfigurationFlags, SharedSerialPortFlags},
		env::{
			ATAPI_DISABLE_LOAD_BEARING_SLEEP, FSEMUL_DISABLE_REMOVAL, PCFS_DISABLE_CSR,
			PCFS_DISABLE_FFIO, PCFS_DISABLE_LOAD_BEARING_SLEEP, PCFS_IS_SATA,
			SDIO_DISABLE_LOAD_BEARING_SLEEP,
		},
	},
};
use cat_dev::mion::{
	cgis::{power_on, power_on_v2},
	proto::cgis::SetupParameters,
};
use miette::miette;
use std::{net::Ipv4Addr, path::PathBuf};
use tokio::task::JoinHandle;
use tracing::{error, field::valuable, info, warn};

/// Actually process the "boot" command, and boot up a cat-dev with all
/// associated servers (if necessary).
pub async fn handle_boot(
	fsemul_flags: FSEmulConfigurationFlags,
	disable_sata: bool,
	no_pcfs: bool,
	serial_port_args: (SharedSerialPortFlags, Option<&PathBuf>),
	parameter_space_port: Option<u16>,
	take_ownership: bool,
) {
	let bridge_ip = get_targeted_bridge_ip().await;
	let bridge_mac = get_targeted_bridge_mac().await;
	let bridge_name = get_targeted_bridge_name().await;
	let host_ip = get_host_bind_address().await;

	let is_modern_bridge = is_modern_bridge(bridge_ip).await;
	let serial_task_handle =
		coalesce_serial_ports(bridge_ip, &serial_port_args.0, serial_port_args.1)
			.await
			.spawn_log_task();

	let (_info_request, setup_params, needs_pcfs) = validate_bridge_ready_for_booting(
		is_modern_bridge,
		take_ownership,
		bridge_ip,
		bridge_mac,
		&bridge_name,
		parameter_space_port,
	)
	.await;

	// TODO(mythra): how can haz ownership?

	if no_pcfs || !needs_pcfs {
		if is_modern_bridge {
			boot_modern_without_pcfs(needs_pcfs, bridge_ip, host_ip, serial_task_handle).await;
		} else {
			boot_legacy_without_pcfs(bridge_ip, serial_task_handle).await;
		}
		return;
	}

	let file_system = lease_host_file_system().await;

	let final_atapi_port = serve_atapi(
		file_system,
		host_ip,
		get_atapi_port().await.or(lease_fsemul_config_optionally()
			.await
			.and_then(|emul| emul.get_atapi_emulation_port())),
		setup_params
			.as_ref()
			.map(SetupParameters::atapi_emulator_port),
		fsemul_flags.disable_load_bearing_sleep_for_atapi() || *ATAPI_DISABLE_LOAD_BEARING_SLEEP,
	)
	.await;
	serve_sdio(
		bridge_ip,
		setup_params.as_ref(),
		file_system,
		get_sdio_control_port().await,
		get_sdio_printf_port().await,
		fsemul_flags.disable_load_bearing_sleep_for_sdio() || *SDIO_DISABLE_LOAD_BEARING_SLEEP,
	)
	.await;
	let will_use_sata = get_will_use_sata(disable_sata);

	let sata_port = if will_use_sata {
		let p = serve_sata(
			file_system,
			host_ip,
			get_pcfs_sata_port()
				.await
				.or(lease_fsemul_config_optionally()
					.await
					.and_then(|emul| emul.get_pcfs_sata_port())),
			fsemul_flags.disable_real_removal() || *FSEMUL_DISABLE_REMOVAL,
			fsemul_flags.disable_ffio() || *PCFS_DISABLE_FFIO,
			fsemul_flags.disable_csr() || *PCFS_DISABLE_CSR,
			fsemul_flags.disable_load_bearing_sleep_for_pcfs() || *PCFS_DISABLE_LOAD_BEARING_SLEEP,
		)
		.await;
		Some(p)
	} else {
		None
	};

	wrap_power_on(
		is_modern_bridge,
		bridge_ip,
		host_ip,
		final_atapi_port,
		sata_port,
	)
	.await;
	_ = serial_task_handle.await;
}

/// Boot without PCFS for a MION running at least FW
/// `0.14.77` (access to `get_info`, and `power_on_v2`).
async fn boot_modern_without_pcfs(
	needs_pcfs: bool,
	bridge_ip: Ipv4Addr,
	host_ip: Option<Ipv4Addr>,
	serial_handle: JoinHandle<()>,
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

	match power_on_v2(bridge_ip, host_ip, None, None, false).await {
		Ok(result_code) => {
			if result_code {
				info!("Successfully powered on MION!");
				_ = serial_handle.await;
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

/// Handling booting without PCFS for legacy mion fw versions aka those
/// below `00.14.77`.
///
/// Legacy mion fw's do not have access to the newer APIs that just let us turn
/// off emulation temporarily, or get info, or do any "smart" things. However,
/// hey we can still turn these things on.
async fn boot_legacy_without_pcfs(bridge_ip: Ipv4Addr, serial_handle: JoinHandle<()>) {
	// We are about to turn on a cat-dev without providing any "niceities" of
	// PCFS. Including the Disc Emulator.
	//
	// As a result we need to ensure the state is set to "no disc in", otherwise
	// the Wii-U components will show an error screen as they think a disc is in
	// the tray, but they can't interact with the disc in anyway.
	turn_down_for_disc(bridge_ip).await;

	match power_on(bridge_ip).await {
		Ok(result_code) => {
			if result_code {
				info!("Successfully powered on MION!");
				_ = serial_handle.await;
			} else {
				error!("Failed to boot cat-dev bridge! Please reach out for support!");
				std::process::exit(BOOT_CGI_FAILURE);
			}
		}
		Err(cause) => {
			if SHOULD_LOG_JSON() {
				error!(
					id = "bridgectl::boot::legacy::failed_to_boot_device",
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

#[must_use]
fn get_will_use_sata(disable_sata_cli: bool) -> bool {
	let result = if disable_sata_cli {
		false
	} else {
		*PCFS_IS_SATA
	};

	if !result {
		if SHOULD_LOG_JSON() {
			warn!(
				id = "bridgectl::boot::disabled_sata_port",
				notes = valuable(&[
					"All File I/O Operations should be expected to be slower, and not as performant.",
				]),
			);
		} else {
			warn!("The SATA server was disabled, all file operations will be noticeably slower.");
		}
	}

	result
}
