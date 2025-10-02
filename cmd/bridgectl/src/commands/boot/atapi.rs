//! Handle the 'ATAPI' related protocols for booting up a MION.
//!
//! ATAPI emulates arbitrary SATA requests for the hard disk, it is DIFFERENT
//! than "PCFS over SATA", which uses it's own other port for PCFS related
//! communications over a similar sata protocol.

use crate::{
	exit_codes::{BOOT_COULD_NOT_CONNECT, BOOT_COULD_NOT_SPAWN},
	knobs::env::ATAPI_OVERRIDE_LOAD_BEARING_SLEEP_MS,
};
use cat_dev::{
	fsemul::{
		HostFilesystem,
		atapi::server::{DEFAULT_ATAPI_PORT, create_atapi_server},
	},
	net::server::TCPServer,
};
use std::net::Ipv4Addr;
use tokio::{signal::ctrl_c as ctrl_c_signal, task::Builder as TaskBuilder};
use tracing::{error, info};

/// Spin up a server ready to handle SATA emulation.
pub async fn serve_atapi(
	host_filesystem: &'static HostFilesystem,
	host_ip: Option<Ipv4Addr>,
	fsemul_atapi_port: Option<u16>,
	setup_params_atapi_port: Option<u16>,
	disable_load_bearing_sleep: bool,
) -> u16 {
	let port = fsemul_atapi_port
		.or(setup_params_atapi_port)
		.unwrap_or(DEFAULT_ATAPI_PORT);
	let atapi_server = match create_atapi_server(
		host_filesystem.clone(),
		host_ip,
		Some(port),
		*ATAPI_OVERRIDE_LOAD_BEARING_SLEEP_MS,
		disable_load_bearing_sleep,
		None,
		false,
		// Can be overriden with env var at the cat-dev level.
		false,
	)
	.await
	{
		Ok(srv) => srv,
		Err(cause) => {
			error!(
				id = "bridgectl::boot::atapi_bind_failure",
				?cause,
				"failed to start server for ATAPI Emulation"
			);

			std::process::exit(BOOT_COULD_NOT_CONNECT);
		}
	};

	spawn_atapi(atapi_server);
	port
}

fn spawn_atapi(atapi_emulator: TCPServer<HostFilesystem>) {
	if let Err(cause) = TaskBuilder::new()
		.name("bridgectl::boot::serve_atapi")
		.spawn(async move {
			tokio::select! {
				res = atapi_emulator.bind() => {
					if let Err(cause) = res {
						error!(
							id = "bridgectl::boot::atapi_spawn_failure",
							?cause,
							"failed to bind server to serve data to MION"
						);
					}
				},
				_ = ctrl_c_signal() => {
					info!(
						id = "bridgectl::boot::atapi_detected_ctrlc",
						"ctrl-c has been hit, shutting down atapi",
					);
				}
			}
		}) {
		error!(
			id = "bridgectl::boot::atapi_spawn_failure",
			?cause,
			"failed to spawn task to serve atapi data to mion"
		);

		std::process::exit(BOOT_COULD_NOT_SPAWN);
	}
}
