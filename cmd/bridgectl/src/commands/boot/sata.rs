//! Handle the 'PCFS Sata' related protocols for booting up a MION.
//!
//! `PCFS` is the main filesystem emulation protocol used by Cafe. As opposed
//! to `FSEmul` which is only available on cat-dev's and does lower filesystem
//! level emulation.

use crate::{
	SHOULD_LOG_JSON,
	exit_codes::{BOOT_COULD_NOT_CONNECT, BOOT_COULD_NOT_SPAWN},
	knobs::env::PCFS_OVERRIDE_LOAD_BEARING_SLEEP_MS,
	utils::add_context_to,
};
use cat_dev::{
	fsemul::{
		HostFilesystem,
		pcfs::sata::server::{PCFSServerState, pcfs_sata_server},
	},
	net::server::TCPServer,
};
use miette::miette;
use std::{net::Ipv4Addr, path::PathBuf};
use tokio::{signal::ctrl_c as ctrl_c_signal, task::Builder as TaskBuilder};
use tracing::{error, info};

/// Spin up a server ready to handle PCFS ovver SATA.
#[allow(
	// CLIPPY ILL THINK ABOUT IT.
	clippy::fn_params_excessive_bools,
	clippy::too_many_arguments,
)]
pub async fn serve_sata(
	host_filesystem: &'static HostFilesystem,
	host_ip: Option<Ipv4Addr>,
	fsemul_sata_port: Option<u16>,
	disable_real_removal: bool,
	disable_ffio: bool,
	disable_csr: bool,
	disable_load_bearing_sleep: bool,
	wal_log_path: Option<PathBuf>,
) -> u16 {
	let sata_server = match pcfs_sata_server(
		host_filesystem.clone(),
		host_ip,
		fsemul_sata_port,
		disable_ffio,
		disable_csr,
		disable_real_removal,
		wal_log_path,
		*PCFS_OVERRIDE_LOAD_BEARING_SLEEP_MS,
		disable_load_bearing_sleep,
		None,
		false,
		// Still settable through env vars.
		false,
	)
	.await
	{
		Ok(srv) => srv,
		Err(cause) => {
			if SHOULD_LOG_JSON() {
				error!(
					id = "bridgectl::boot::sata_bind_failure",
					?cause,
					"failed to start server for PCFS Sata Emulation"
				);
			} else {
				error!(
					"\n{:?}",
					add_context_to(
						miette!("Failed to start server for PCFS Sata Emulation"),
						[cause.into()].into_iter(),
					),
				);
			}

			std::process::exit(BOOT_COULD_NOT_CONNECT);
		}
	};

	let port = sata_server.port();
	spawn_sata(sata_server);
	port
}

fn spawn_sata(sata_emulator: TCPServer<PCFSServerState>) {
	if let Err(cause) = TaskBuilder::new()
		.name("bridgectl::boot::serve_pcfs::sata")
		.spawn(async move {
			tokio::select! {
				_res = sata_emulator.bind() => {}
				_ = ctrl_c_signal() => if SHOULD_LOG_JSON() {
					info!(
						id = "bridgectl::boot::sata_detected_ctrlc",
						"ctrl-c has been hit, shutting down PCFS over sata",
					);
				} else {
					info!(
						"Ctrl-C has been detected as being hit! Shutting down sata!"
					);
				}
			}
		}) {
		if SHOULD_LOG_JSON() {
			error!(
				id = "bridgectl::boot::pcfs_sata_spawn_failure",
				?cause,
				"failed to spawn task to serve pcfs sata data to mion"
			);
		} else {
			error!(
				"\n{:?}",
				add_context_to(
					miette!("Failed to spawn task to serve PCFS Sata data to MION!"),
					[miette!("{cause:?}")].into_iter(),
				),
			);
		}

		std::process::exit(BOOT_COULD_NOT_SPAWN);
	}
}
