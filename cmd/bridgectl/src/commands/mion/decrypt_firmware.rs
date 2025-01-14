//! Handle decrypting a MION firmware file distributed as part of the Cafe SDK.

use crate::{
	exit_codes::{
		DECRYPT_FW_BAD_OUTPUT_PATH, DECRYPT_FW_COULD_NOT_DECRYPT_FW, DECRYPT_FW_COULD_NOT_READ_FW,
		DECRYPT_FW_COULD_NOT_WRITE_FW,
	},
	utils::add_context_to,
	SHOULD_LOG_JSON,
};
use cat_dev::mion::firmware::raw_decrypt;
use miette::{miette, IntoDiagnostic};
use std::path::{Path, PathBuf};
use tracing::{error, field::valuable};

/// Actually handle the `mion decrypt-fw` command.
pub async fn handle_decrypt_firmware(
	firmware_file: PathBuf,
	output_path_flag: Option<PathBuf>,
	output_path_positional: Option<PathBuf>,
) {
	let output_path = get_output_path(&firmware_file, output_path_flag, output_path_positional);

	let encrypted_firmware_contents = match tokio::fs::read(&firmware_file).await.into_diagnostic()
	{
		Ok(bytes) => bytes,
		Err(cause) => {
			if SHOULD_LOG_JSON() {
				error!(
					id = "bridgectl::mion::decrypt_firmware::could_not_read_firmware_file",
					?cause,
					file = valuable(&firmware_file),
					"Could not read firmware file to decrypt.",
				);
			} else {
				error!(
					"\n{:?}",
					add_context_to(
						miette!(
    					"Could not read the firmware file to decrypt, perhaps some filesystem error?"
    				),
						[
							cause,
							miette!(format!("Was Reading File: {}", firmware_file.display())),
						]
						.into_iter(),
					),
				);
			}

			std::process::exit(DECRYPT_FW_COULD_NOT_READ_FW);
		}
	};

	// Decrypt the main body of the firmware without validation.
	let mut decrypted_contents =
		match raw_decrypt(&encrypted_firmware_contents[..encrypted_firmware_contents.len() - 6]) {
			Ok(contents) => contents,
			Err(cause) => {
				if SHOULD_LOG_JSON() {
					error!(
						id = "bridgectl::mion::decrypt_firmware::could_not_decrypt",
						?cause,
						file = valuable(&firmware_file),
						"Could not decrypt the firmware file, must be corrupt.",
					);
				} else {
					error!(
						"\n{:?}",
						add_context_to(
							miette!(
    						"Could not decrypt the firmware file, perhaps it is corrupt in some way?"
    					),
							[
								cause.into(),
								miette!(format!("Was Reading File: {}", firmware_file.display())),
							]
							.into_iter(),
						),
					);
				}

				std::process::exit(DECRYPT_FW_COULD_NOT_DECRYPT_FW);
			}
		};

	// Now append the footer information which isn't encrypted.
	decrypted_contents
		.extend_from_slice(&encrypted_firmware_contents[&encrypted_firmware_contents.len() - 6..]);

	// Now we can write!
	if let Err(cause) = tokio::fs::write(&output_path, decrypted_contents)
		.await
		.into_diagnostic()
	{
		if SHOULD_LOG_JSON() {
			error!(
				id = "bridgectl::mion::decrypt_firmware::could_not_write_decrypted_firmware_file",
				?cause,
				output_file = valuable(&output_path),
				"Could not write decrypted file contents.",
			);
		} else {
			error!(
				"\n{:?}",
				add_context_to(
					miette!(
    				"Could not write the decrypted firmware file, perhaps some filesystem error?"
    			),
					[
						cause,
						miette!(format!("Was Writing File: {}", output_path.display())),
					]
					.into_iter(),
				),
			);
		}

		std::process::exit(DECRYPT_FW_COULD_NOT_WRITE_FW);
	}
}

fn get_output_path(
	firmware_file: &Path,
	output_path_flag: Option<PathBuf>,
	output_path_positional: Option<PathBuf>,
) -> PathBuf {
	if (output_path_flag.is_some() && output_path_positional.is_some())
		&& output_path_flag != output_path_positional
	{
		if SHOULD_LOG_JSON() {
			error!(
				id = "bridgectl::mion::decrypt_firmware::conflicting_output_paths",
				flag.output = valuable(&output_path_flag),
				argument.output = valuable(&output_path_positional),
				"Specified the Output Path twice with different values, not sure where to output.",
			);
		} else {
			error!(
				flag.output = valuable(&output_path_flag),
				argument.output = valuable(&output_path_positional),
				"Specified the Output Path twice with different values, not sure where to output.",
			);
		}

		std::process::exit(DECRYPT_FW_BAD_OUTPUT_PATH);
	}

	if let Some(flag) = output_path_flag {
		flag
	} else if let Some(positional) = output_path_positional {
		positional
	} else {
		let mut os_str = firmware_file.as_os_str().to_os_string();
		os_str.push(".decrypted");
		PathBuf::from(os_str)
	}
}
