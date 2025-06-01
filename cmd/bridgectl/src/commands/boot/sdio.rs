//! Handle the 'SDIO' related protocols for booting up a MION.
//!
//! SDIO is one of the many protocols you might classify as 'part of PCFS', it
//! is one way to transfer files/blocks form the PC to the filesystem.

use crate::{
	SHOULD_LOG_JSON,
	exit_codes::{BOOT_COULD_NOT_CONNECT, BOOT_COULD_NOT_SPAWN, BOOT_FSEMUL_SERVER_ERROR},
	knobs::env::SDIO_OVERRIDE_LOAD_BEARING_SLEEP_MS,
	utils::add_context_to,
};
use cat_dev::{
	fsemul::{
		HostFilesystem,
		sdio::server::{SDIOStreamState, sdio_server},
	},
	mion::proto::cgis::SetupParameters,
	net::server::TCPServer,
};
use miette::miette;
use std::net::Ipv4Addr;
use tokio::{signal::ctrl_c as ctrl_c_signal, task::Builder as TaskBuilder};
use tracing::{error, field::valuable, info};

/// Connect to the bridge, and start serving SDIO data as needed for PCFS.
///
/// Yes although the CAT-DEV is mostly sending _us_ requests, we don't actually
/// spawn a server, and have the cat-dev connect to us. Instead we connect to
/// them and then basically act as a server.
///
/// ## Exits
///
/// - If we cannot connect to the SDIO port of the cat-dev machine.
/// - If we cannot spawn a background task to start processing SDIO requests.
pub async fn serve_sdio(
	bridge_ip: Ipv4Addr,
	setup_params: Option<&SetupParameters>,
	host_file_system: &'static HostFilesystem,
	override_control_port: Option<u16>,
	override_printf_port: Option<u16>,
	disable_load_bearing_sleep: bool,
) {
	let opt_printf_port =
		override_printf_port.or(setup_params.map(SetupParameters::sdio_printf_port));
	let opt_data_port =
		override_control_port.or(setup_params.map(SetupParameters::sdio_block_port));

	let sdio_client = match sdio_server(
		bridge_ip,
		opt_printf_port,
		opt_data_port,
		host_file_system,
		*SDIO_OVERRIDE_LOAD_BEARING_SLEEP_MS,
		disable_load_bearing_sleep,
		None,
		false,
		// Can be overriden with an environt variable.
		false,
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
							miette!(
								"Please ensure the device is running and has no error lights on."
							),
							miette!(
								"A reboot of the cat-dev device, and letting it settle may fix this."
							),
							cause.into(),
						]
						.into_iter(),
					),
				);
			}

			std::process::exit(BOOT_COULD_NOT_CONNECT);
		}
	};

	spawn_sdio_task(sdio_client);
}

/// Actually spawn the background task that will process incoming requests,
/// responses from the SDIO ports of MION communication.
fn spawn_sdio_task(sdio_client: TCPServer<SDIOStreamState>) {
	if let Err(cause) = TaskBuilder::new().name("bridgectl::boot::serve_sdio").spawn(async move {
		tokio::select! {
			result = sdio_client.connect() => {
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
