//! Utilities that aren't directly related to booting, but help get info that
//! can inform the booting process.

use crate::exit_codes::{
	BOOT_CGI_FAILURE, BOOT_COULD_NOT_CONNECT, BOOT_NOT_READY_TO_BOOT, BRIDGE_TOO_OLD_FOR_FEATURE,
};
use cat_dev::mion::{
	cgis::{
		get_info, get_setup_parameters, get_versions, power_on, power_on_v2, set_disc_eject_state,
	},
	parameter::get_parameters as get_param_space_parameters,
	proto::{cgis::SetupParameters, control::MIONBootType},
};
use fnv::FnvHashMap;
use mac_address::MacAddress;
use std::{hash::BuildHasherDefault, net::Ipv4Addr};
use tracing::{debug, error, warn};

/// Determine if we are running a "modern" bridge.
///
/// A modern bridge is one which has support for all the features of boot
/// (lukcily there is no bridge that has 'some' but not 'all' of the features,
/// at least not that we've found *knocks on wood*). The cutoff version we have
/// is `0.00.14.77`. Anyv ersion running AT LEAST that Firmware, or above is
/// considered to be a "modern bridge".
///
/// You can check this on your MION by visiting your MIONs website at:
/// `http://<mion-ip>/update.cgi`.
///
/// This function will also log a warning if you're not running a modern bridge
/// so that way folks know what's going on.
///
/// ## Exits
///
/// This function will exit the boot command if we cannot parse out the
/// versions from `http://<mion ip>/update.cgi`, either cause we didn't make
/// a successful HTTP request, or it contained data we didn't understand.
pub async fn is_modern_bridge(bridge_ip: Ipv4Addr) -> bool {
	debug!(
		id = "bridgectl::boot::check_modern_bridge",
		"Determining MION capabilities...",
	);

	let versions = match get_versions(bridge_ip).await {
		Ok(versions) => versions,
		Err(cause) => {
			error!(
				id = "bridgectl::boot::failed_to_get_mion_version",
				?cause,
				bridge.ip = %bridge_ip,
				"Failed to get MION version!",
			);

			std::process::exit(BOOT_COULD_NOT_CONNECT);
		}
	};
	let mion_version = versions.get_mion_version();

	// Assume all mion versions past 0.14.77 implement the necessary `get_info`,
	// and `power_on_v2` APIs.
	let is_modern = mion_version[0] >= 1
		|| mion_version[1] > 14
		|| (mion_version[1] == 14 && mion_version[2] >= 77);

	if !is_modern {
		warn!(
			id = "bridgectl::boot::old_mion_warning",
			bridge.ip = %bridge_ip,
			bridge.version = %versions.displayable_mion_version(),
			"Your Cat-DEV's MION Board Version is rather old, and not fully functional. Please update it to at least 0.00.14.77 for full functionality...",
		);
	}

	is_modern
}

/// Perform preflight checks to confirm a bridge is ready to actually be
/// powered on, and info necessary to power on the bridge.
///
/// The real only "check" here is validating that the machine is not owned
/// by another host. This is also only supported on more modern MIONs. On
/// older MIONs we just pray and hope that it's not already being used.
///
/// On Older MIONs we could in theory check for "power", but 'power' state does
/// NOT correlate with ownership. There is no way to check for true ownership
/// besides just actually trying to connect, and do the boot.
///
/// ## Exits
///
/// If you are trying to take ownership of a bridge when it's running legacy
/// firmware (only newer mion fw's support takeover).
///
/// If we cannot get the necessary information to actually boot the device.
#[must_use]
pub async fn validate_bridge_ready_for_booting(
	is_modern_bridge: bool,
	will_take_over: bool,
	bridge_ip: Ipv4Addr,
	bridge_mac: MacAddress,
	bridge_name: &str,
	parameter_space_port: Option<u16>,
) -> (FnvHashMap<String, String>, Option<SetupParameters>, bool) {
	if !is_modern_bridge {
		return validate_legacy_bridge_ready_for_booting(
			will_take_over,
			bridge_ip,
			bridge_mac,
			bridge_name,
			parameter_space_port,
		)
		.await;
	}

	let (information_request, setup_parameters) =
		get_info_and_parameters(bridge_ip, bridge_name).await;
	if information_request
		.get("RESULT")
		.map(String::as_str)
		.unwrap_or_default()
		!= "OK"
	{
		error!(
			id = "bridgectl::boot::check_info_result",
			response_fields = ?information_request,
			"Result was not okay for receiving information",
		);

		std::process::exit(BOOT_NOT_READY_TO_BOOT);
	}

	if information_request
		.get("curhost")
		.map(String::as_str)
		.unwrap_or_default()
		!= "0.0.0.0"
	{
		warn!(
			id = "bridgectl::boot::owned_by_other_host",
			bridge.ip = %bridge_ip,
			bridge.name = %bridge_name,
			response_fields = ?information_request,
			"MION is currently being managed by another host!!",
		);

		if !will_take_over {
			error!(
				id = "bridgectl::boot::mion_already_owned",
				bridge.ip = %bridge_ip,
				bridge.name = %bridge_name,
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

/// "Eject", or turn down the signal for the disc so the MION does not think a
/// device is actively inserted.
///
/// ## Exits
///
/// If we cannot make the HTTP request, or parse the response for ejecting the
/// disc. Will also error if the device said it did not return okay when
/// ejecting the disc.
pub async fn turn_down_for_disc(bridge_ip: Ipv4Addr) {
	match set_disc_eject_state(bridge_ip, false).await {
		Ok(success) => {
			if !success {
				error!(
					id = "bridgectl::boot::eject_failed",
					bridge.ip = %bridge_ip,
					"Bridge responded with an error (and no extra info) while ejecting the disc",
				);

				std::process::exit(BOOT_NOT_READY_TO_BOOT);
			}
		}
		Err(cause) => {
			error!(
				id = "bridgectl::boot::cannot_eject_disc",
				?cause,
				bridge.ip = %bridge_ip,
				"cannot eject disc to properly boot bridge",
			);

			std::process::exit(BOOT_COULD_NOT_CONNECT);
		}
	}
}

/// Call the POWER ON for this particular device. If we're talking to a modern
/// bridge we'll use [`power_on_v2`], otherwise [`power_on`].
///
/// ## Exits
///
/// If we cannot make the HTTP request, or parse the response for powering on.
/// Will also error if the device said it did not return okay when powering on.
pub async fn wrap_power_on(
	is_modern_bridge: bool,
	bridge_ip: Ipv4Addr,
	host_ip: Option<Ipv4Addr>,
	final_atapi_port: u16,
	final_sata_port: Option<u16>,
) {
	let res = if is_modern_bridge {
		power_on_v2(
			bridge_ip,
			host_ip,
			Some(final_atapi_port),
			final_sata_port,
			true,
		)
		.await
	} else {
		power_on(bridge_ip).await.map_err(Into::into)
	};

	if let Err(cause) = res {
		error!(
			id = "bridgectl::boot::send_power_on",
			?cause,
			bridge.ip = %bridge_ip,
			bridge.is_modern = is_modern_bridge,
			host.ip_override = ?host_ip,
			host.atapi_port = %final_atapi_port,
			host.sata_port = ?final_sata_port,
			"Failed to power on bridge!",
		);

		std::process::exit(BOOT_CGI_FAILURE);
	}
}

/// Validate if a bridge is ready to be booted when running legacy MION
/// Firmwares.
///
/// Legacy MION Firmwares do _not_ have support for "ownership takeover", which
/// more modern MION FW's allowed for. They also don't have a `get_info`
/// endpoint which can return useful information.
///
/// So in this case we just fetch the setup parameters necessary for booting,
/// and stub out everything else.
async fn validate_legacy_bridge_ready_for_booting(
	will_take_over: bool,
	bridge_ip: Ipv4Addr,
	bridge_mac: MacAddress,
	bridge_name: &str,
	parameter_space_port: Option<u16>,
) -> (FnvHashMap<String, String>, Option<SetupParameters>, bool) {
	if will_take_over {
		error!(
			id = "bridgectl::boot::take_over_unsupported",
			"you specified `--take-ownership` which is not supported for MIONs running this old of FW..."
		);

		std::process::exit(BRIDGE_TOO_OLD_FOR_FEATURE);
	}
	let setup_parameters =
		wrap_get_setup_parameters(bridge_ip, bridge_name, Some(bridge_mac)).await;

	let mion_space_params =
		match get_param_space_parameters(bridge_ip, parameter_space_port, None).await {
			Ok(ps) => ps,
			Err(cause) => {
				error!(
					id = "bridgectl::boot::failed_dump_mion_param_space",
					?cause,
					bridge.ip = %bridge_ip,
					bridge.override_ps_port = ?parameter_space_port,
					"Failed to get boot mode for legacy MION of parameter space port.",
				);

				std::process::exit(BOOT_COULD_NOT_CONNECT);
			}
		};
	let boot_mode = MIONBootType::from(
		mion_space_params
			.get_parameter_by_index(2)
			.unwrap_or_default(),
	);

	(
		FnvHashMap::with_capacity_and_hasher(0, BuildHasherDefault::default()),
		setup_parameters,
		matches!(boot_mode, MIONBootType::PCFS | MIONBootType::DUAL),
	)
}

/// On a modern bridge perform `get_info` to get extra information, and the
/// setup parameters for the device.
///
/// Wrap both operations into one simple function.
async fn get_info_and_parameters(
	bridge_ip: Ipv4Addr,
	bridge_name: &str,
) -> (FnvHashMap<String, String>, Option<SetupParameters>) {
	let information_request = match get_info(bridge_ip, bridge_name).await {
		Ok(map) => map,
		Err(cause) => {
			error!(
				id = "bridgectl::boot::get_bridge_info",
				?cause,
				help = format!(
					"You can ensure the bridge is ready to be viewed at: <http://{bridge_ip}/menu.cgi>"
				),
				"failed to query bridge information to make sure it was ready for booting",
			);

			std::process::exit(BOOT_CGI_FAILURE);
		}
	};
	let setup_parameters = wrap_get_setup_parameters(
		bridge_ip,
		bridge_name,
		information_request
			.get("mac")
			.and_then(|val| val.parse::<MacAddress>().ok()),
	)
	.await;

	(information_request, setup_parameters)
}

/// Small wrapper around `get_setup_parameters` to exit gracefully if we cannot
/// populate a value.
async fn wrap_get_setup_parameters(
	bridge_ip: Ipv4Addr,
	bridge_name: &str,
	default_mac: Option<MacAddress>,
) -> Option<SetupParameters> {
	match get_setup_parameters(bridge_ip).await {
		Ok(params) => Some(params),
		Err(cause) => {
			error!(
				id = "bridgectl::boot::get_setup_parameters",
				?cause,
				help = format!("You can see the setup page at: <http://{bridge_ip}/setup.cgi>"),
				"failed to query setup parameters to make sure the bridge was ready for booting",
			);

			if let Some(mac) = default_mac {
				warn!("Using default setup-parameters, may be incorrect");
				Some(SetupParameters::default_settings(
					bridge_name.to_owned(),
					mac,
				))
			} else {
				None
			}
		}
	}
}
