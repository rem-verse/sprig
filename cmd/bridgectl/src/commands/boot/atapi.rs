//! Handle the 'ATAPI' related protocols for booting up a MION.
//!
//! ATAPI emulates arbitrary SATA requests for the hard disk, it is DIFFERENT
//! than "PCFS over SATA", which uses it's own other port for PCFS related
//! communications over a similar sata protocol.

use crate::{
	SHOULD_LOG_JSON,
	exit_codes::{BOOT_COULD_NOT_CONNECT, BOOT_COULD_NOT_SPAWN},
	utils::add_context_to,
};
use cat_dev::fsemul::{
	HostFilesystem,
	atapi::server::{AtapiServer, DEFAULT_ATAPI_PORT},
};
use miette::miette;
use std::net::Ipv4Addr;
use tokio::{signal::ctrl_c as ctrl_c_signal, task::Builder as TaskBuilder};
use tracing::{error, info};

/// Spin up a server ready to handle SATA emulation.
pub async fn serve_atapi(
	host_filesystem: &'static HostFilesystem,
	host_ip: Option<Ipv4Addr>,
	fsemul_atapi_port: Option<u16>,
	setup_params_atapi_port: Option<u16>,
) -> u16 {
	let port = fsemul_atapi_port
		.or(setup_params_atapi_port)
		.unwrap_or(DEFAULT_ATAPI_PORT);
	let atapi_server = match AtapiServer::new(host_filesystem, host_ip, Some(port)).await {
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
	port
}

fn spawn_atapi(atapi_emulator: AtapiServer<'static>) {
	if let Err(cause) = TaskBuilder::new()
		.name("bridgectl::boot::serve_atapi")
		.spawn(async move {
			tokio::select! {
				() = atapi_emulator.serve_concurrently() => {}
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
