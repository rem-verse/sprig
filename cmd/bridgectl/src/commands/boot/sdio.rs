//! Handle the 'SDIO' related protocols for booting up a MION.
//!
//! SDIO is one of the many protocols you might classify as 'part of PCFS', it
//! is one way to transfer files/blocks form the PC to the filesystem.

use crate::{
	exit_codes::{BOOT_COULD_NOT_CONNECT, BOOT_COULD_NOT_SPAWN, BOOT_FSEMUL_SERVER_ERROR},
	knobs::env::SDIO_OVERRIDE_LOAD_BEARING_SLEEP_MS,
};
use cat_dev::{
	fsemul::{
		HostFilesystem,
		sdio::server::{SDIOStreamState, sdio_server},
	},
	mion::proto::cgis::SetupParameters,
	net::server::TCPServer,
};
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
			error!(
				id = "bridgectl::boot::sdio_connection_failure",
				?cause,
				help = valuable(&[
					"Please ensure the device is running, and has no error lights on.",
					"A reboot of the cat-dev device, and letting it settle may fix this.",
				]),
				"failed to connect to cat-dev to serve sdio data"
			);

			std::process::exit(BOOT_COULD_NOT_CONNECT);
		}
	};

	spawn_sdio_task(sdio_client);
}

/// Actually spawn the background task that will process incoming requests,
/// responses from the SDIO ports of MION communication.
fn spawn_sdio_task(sdio_client: TCPServer<SDIOStreamState>) {
	if let Err(cause) = TaskBuilder::new()
		.name("bridgectl::boot::serve_sdio")
		.spawn(async move {
			tokio::select! {
				result = sdio_client.connect() => {
					if let Err(cause) = result {
						error!(
							id = "bridgectl::boot::sdio_failed_to_serve",
							?cause,
							"failed serving sdio data to mion for pcfs",
						);

						std::process::exit(BOOT_FSEMUL_SERVER_ERROR);
					} else {
						info!(
							id = "bridgectl::boot::sdio_exited_gracefully",
							"SDIO has gracefully requested termination.",
						);
					}
				}
				_ = ctrl_c_signal() => {
					info!(
						id = "bridgectl::boot::sdio_detected_ctrlc",
						"ctrl-c has been hit, shutting down sdio",
					);
				}
			}
		}) {
		error!(
			id = "bridgectl::boot::sdio_spawn_failure",
			?cause,
			help = valuable(&[
				"Please ensure another copy of this program isn't running!",
				"Please ensure you have enough resources to handle this server!",
			]),
			"failed to spawn task to serve sdio data to mion"
		);

		std::process::exit(BOOT_COULD_NOT_SPAWN);
	}
}
