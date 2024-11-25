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
//! TODO(cynthia): check 0.14.77 Firmware for ability to do `get_info` & `power_on_v2`
//!
//! The booting process is pretty heavily impacted by the version of the Mion
//! FW you are running. For example only the latest firmware we have dumped
//! `0.00.14.80` has support for "taking over" a MION. Any firmware before that
//! does not have the capability to do so.
//!
//! The raw Power ON APIs are also not as configurable when running versions
//! pre `0.00.14.80`, where `power_on_v2` exists.

use crate::{
	commands::argv_helpers::{
		coalesce_serial_ports, get_host_bind_address, get_targeted_bridge_ip,
		get_targeted_bridge_name, lease_fsemul_config_optionally, lease_host_file_system,
		spawn_serial_log_task,
	},
	exit_codes::{
		BOOT_CGI_FAILURE, BOOT_COULD_NOT_CONNECT, BOOT_COULD_NOT_SPAWN, BOOT_FSEMUL_SERVER_ERROR,
		BOOT_NOT_READY_TO_BOOT,
	},
	knobs::env::{CONNECTION_TIMEOUT, PCFS_IS_SATA},
	utils::add_context_to,
	SHOULD_LOG_JSON,
};
use cat_dev::{
	fsemul::{atapi::AtapiServer, sdio::SdioClient, HostFilesystem},
	mion::{
		cgis::{get_info, get_setup_parameters, power_on_v2},
		proto::{cgis::SetupParameters, control::MIONBootType},
	},
};
use fnv::FnvHashMap;
use mac_address::MacAddress;
use miette::miette;
use std::{net::Ipv4Addr, path::PathBuf};
use tokio::{
	signal::ctrl_c as ctrl_c_signal,
	task::{Builder as TaskBuilder, JoinHandle},
};
use tracing::{error, field::valuable, info, warn};

pub async fn handle_boot(
	disable_sata: bool,
	no_pcfs: bool,
	serial_port_args: (Option<PathBuf>, Option<PathBuf>),
	take_ownership: bool,
) {
	let bridge_ip = get_targeted_bridge_ip().await;
	let bridge_name = get_targeted_bridge_name().await;
	let host_ip = get_host_bind_address().await;

	let mut serial_task_handle = None;
	if let Some((serial_port, serial_path)) =
		coalesce_serial_ports(serial_port_args.0.as_ref(), serial_port_args.1.as_ref())
	{
		serial_task_handle = Some(spawn_serial_log_task(serial_port, serial_path));
	}
	let (_info_request, setup_params, needs_pcfs) =
		validate_bridge_ready_for_booting(take_ownership, bridge_ip, &bridge_name).await;

	// TODO(mythra): can take ownership? through set param?

	if no_pcfs || !needs_pcfs {
		boot_without_pcfs(needs_pcfs, bridge_ip, host_ip, serial_task_handle).await;
		return;
	}

	let fsemul = lease_fsemul_config_optionally().await;
	let file_system = lease_host_file_system().await;
	serve_atapi(
		host_ip,
		fsemul.and_then(|emul| emul.get_atapi_emulation_port()),
		setup_params
			.as_ref()
			.map(SetupParameters::atapi_emulator_port),
	)
	.await;
	serve_sdio(bridge_ip, setup_params.as_ref(), file_system).await;
	let _will_use_sata = get_will_use_sata(disable_sata);
}

async fn boot_without_pcfs(
	needs_pcfs: bool,
	bridge_ip: Ipv4Addr,
	host_ip: Option<Ipv4Addr>,
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

	match power_on_v2(bridge_ip, host_ip, None, None, false).await {
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
) -> (FnvHashMap<String, String>, Option<SetupParameters>, bool) {
	let (information_request, setup_parameters) =
		get_info_and_parameters(bridge_ip, bridge_name).await;
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

	let boot_mode = MIONBootType::from(
		information_request
			.get("bootmode")
			.map(String::as_str)
			.unwrap_or_default()
			.parse::<u8>()
			.unwrap_or(u8::MAX),
	);

	(
		information_request,
		setup_parameters,
		matches!(boot_mode, MIONBootType::DUAL | MIONBootType::PCFS),
	)
}

/// Connect to the bridge, and start serving SDIO data as needed for PCFS.
async fn serve_sdio(
	bridge_ip: Ipv4Addr,
	setup_params: Option<&SetupParameters>,
	host_file_system: &'static HostFilesystem,
) {
	let opt_printf_port = setup_params.map(SetupParameters::sdio_printf_port);
	let opt_data_port = setup_params.map(SetupParameters::sdio_block_port);

	let sdio_client = match SdioClient::connect(
		bridge_ip,
		opt_printf_port,
		opt_data_port,
		*CONNECTION_TIMEOUT,
		host_file_system,
	)
	.await
	{
		Ok(client) => client,
		Err(cause) => {
			if SHOULD_LOG_JSON() {
				error!(
					id = "bridgectl::boot::sdio_connection_failure",
					?cause,
					help = valuable(&[
						"Please ensure the device is running, and has no error lights on.",
						"A reboot of the cat-dev device, and letting it settle may fix this.",
					]),
					"failed to connect to cat-dev to serve sdio data"
				);
			} else {
				error!(
					"\n{:?}",
					add_context_to(
						miette!("Failed to connect to cat-dev to serve data over SDIO"),
						[
							miette!("Please ensure the device is running and has no error lights on."),
							miette!("A reboot of the cat-dev device, and letting it settle may fix this."),
							cause.into(),
						].into_iter(),
					),
				);
			}

			std::process::exit(BOOT_COULD_NOT_CONNECT);
		}
	};

	spawn_sdio_task(sdio_client);
}

/// Spin up a server ready to handle disc emulation.
async fn serve_atapi(
	host_ip: Option<Ipv4Addr>,
	fsemul_atapi_port: Option<u16>,
	setup_params_atapi_port: Option<u16>,
) {
	let atapi_server =
		match AtapiServer::new(host_ip, fsemul_atapi_port.or(setup_params_atapi_port)).await {
			Ok(srv) => srv,
			Err(cause) => {
				if SHOULD_LOG_JSON() {
					error!(
						id = "bridgectl::boot::atapi_bind_failure",
						?cause,
						"failed to start server for ATAPI Emulation"
					);
				} else {
					error!(
						"\n{:?}",
						add_context_to(
							miette!("Failed to start server for ATAPI Emulation"),
							[cause.into()].into_iter(),
						),
					);
				}

				std::process::exit(BOOT_COULD_NOT_CONNECT);
			}
		};

	spawn_atapi(atapi_server);
}

async fn get_info_and_parameters(
	bridge_ip: Ipv4Addr,
	bridge_name: &str,
) -> (FnvHashMap<String, String>, Option<SetupParameters>) {
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

	let setup_parameters = match get_setup_parameters(bridge_ip).await {
		Ok(params) => Some(params),
		Err(cause) => {
			if SHOULD_LOG_JSON() {
				error!(
					id = "bridgectl::boot::get_setup_parameters",
					?cause,
					"failed to query setup parameters to make sure the bridge was ready for booting",
				);
			} else {
				error!(
					"\n{:?}",
					miette!(
						help = format!("You can see the setup page at: <http://{bridge_ip}/setup.cgi>"),
						"Failed to query setup information to make sure it was ready for booting, please make sure that the device is on, and has it's webpages running.",
					).wrap_err(cause),
				);
			}

			if let Some(mac) = information_request
				.get("mac")
				.and_then(|val| val.parse::<MacAddress>().ok())
			{
				warn!("Using default setup-parameters, may be incorrect");
				Some(SetupParameters::default_settings(
					bridge_name.to_owned(),
					mac,
				))
			} else {
				None
			}
		}
	};

	(information_request, setup_parameters)
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

fn spawn_sdio_task(sdio_client: SdioClient<'static>) {
	if let Err(cause) = TaskBuilder::new().name("bridgectl::boot::serve_sdio").spawn(async move {
		tokio::select! {
			result = sdio_client.serve_concurrently() => {
				if let Err(cause) = result {
					if SHOULD_LOG_JSON() {
						error!(
							id = "bridgectl::boot::sdio_failed_to_serve",
							?cause,
							"failed serving sdio data to mion for pcfs",
						);
					} else {
						error!(
							"\n{:?}",
							add_context_to(
								miette!("Failed serving SDIO data to MION for PCFS"),
								[
									miette!("This may mean the error light will start flashing/boot will fail because we cannot serve the proper data to your device."),
									cause.into(),
								].into_iter(),
							),
						);
					}

					std::process::exit(BOOT_FSEMUL_SERVER_ERROR);
				} else if SHOULD_LOG_JSON() {
					info!(
						id = "bridgectl::boot::sdio_exited_gracefully",
						"SDIO has gracefully requested termination.",
					);
				} else {
					info!("Shutting down SDIO gracefully");
				}
			}
			_ = ctrl_c_signal() => if SHOULD_LOG_JSON() {
				info!(
					id = "bridgectl::boot::sdio_detected_ctrlc",
					"ctrl-c has been hit, shutting down sdio",
				);
			} else {
				info!(
					"Ctrl-C has been detected as being hit! Shutting down SDIO!"
				);
			}
		}
	}) {
		if SHOULD_LOG_JSON() {
			error!(
				id = "bridgectl::boot::sdio_spawn_failure",
				?cause,
				"failed to spawn task to serve sdio data to mion"
			);
		} else {
			error!(
				"\n{:?}",
				add_context_to(
					miette!("Failed to spawn task to serve SDIO data to MION!"),
					[
						miette!("Please ensure another copy of this program isn't running!"),
						miette!("Please ensure you have enough resources to handle this server!"),
						miette!("{cause:?}"),
					].into_iter(),
				),
			);
		}

		std::process::exit(BOOT_COULD_NOT_SPAWN);
	}
}

fn spawn_atapi(atapi_emulator: AtapiServer) {
	if let Err(cause) = TaskBuilder::new()
		.name("bridgectl::boot::serve_atapi")
		.spawn(async move {
			tokio::select! {
				() = atapi_emulator.serve() => {}
				_ = ctrl_c_signal() => if SHOULD_LOG_JSON() {
					info!(
						id = "bridgectl::boot::atapi_detected_ctrlc",
						"ctrl-c has been hit, shutting down atapi",
					);
				} else {
					info!(
						"Ctrl-C has been detected as being hit! Shutting down atapi!"
					);
				}
			}
		}) {
		if SHOULD_LOG_JSON() {
			error!(
				id = "bridgectl::boot::atapi_spawn_failure",
				?cause,
				"failed to spawn task to serve atapi data to mion"
			);
		} else {
			error!(
				"\n{:?}",
				add_context_to(
					miette!("Failed to spawn task to serve ATAPI data to MION!"),
					[miette!("{cause:?}")].into_iter(),
				),
			);
		}

		std::process::exit(BOOT_COULD_NOT_SPAWN);
	}
}
