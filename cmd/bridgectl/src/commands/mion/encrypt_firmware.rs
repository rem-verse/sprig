//! Handle encrypting a MION firmware file distributed as part of the Cafe SDK.

use crate::exit_codes::{
	ENCRYPT_FW_BAD_OUTPUT_PATH, ENCRYPT_FW_COULD_NOT_ENCRYPT_FW, ENCRYPT_FW_COULD_NOT_READ_FW,
	ENCRYPT_FW_COULD_NOT_WRITE_FW,
};
use cat_dev::mion::firmware::raw_encrypt;
use miette::IntoDiagnostic;
use std::path::{Path, PathBuf};
use tracing::{error, field::valuable};

/// Actually handle the `mion encrypt-fw` command.
pub async fn handle_encrypt_firmware(
	firmware_file: PathBuf,
	output_path_flag: Option<PathBuf>,
	output_path_positional: Option<PathBuf>,
) {
	let output_path = get_output_path(&firmware_file, output_path_flag, output_path_positional);

	let decrypted_firmware_contents = match tokio::fs::read(&firmware_file).await.into_diagnostic()
	{
		Ok(bytes) => bytes,
		Err(cause) => {
			error!(
				id = "bridgectl::mion::encrypt_firmware::could_not_read_firmware_file",
				?cause,
				file = valuable(&firmware_file),
				"Could not read firmware file to encrypt.",
			);

			std::process::exit(ENCRYPT_FW_COULD_NOT_READ_FW);
		}
	};

	// Encrypt the main body of the firmware without validation.
	let mut encrypted_contents =
		match raw_encrypt(&decrypted_firmware_contents[..decrypted_firmware_contents.len() - 6]) {
			Ok(contents) => contents,
			Err(cause) => {
				error!(
					id = "bridgectl::mion::encrypt_firmware::could_not_encrypt",
					?cause,
					file = valuable(&firmware_file),
					"Could not encrypt the firmware file, must be corrupt.",
				);

				std::process::exit(ENCRYPT_FW_COULD_NOT_ENCRYPT_FW);
			}
		};

	// Now append the footer information which isn't encrypted.
	encrypted_contents
		.extend_from_slice(&decrypted_firmware_contents[&decrypted_firmware_contents.len() - 6..]);

	// Now we can write!
	if let Err(cause) = tokio::fs::write(&output_path, encrypted_contents)
		.await
		.into_diagnostic()
	{
		error!(
			id = "bridgectl::mion::encrypt_firmware::could_not_write_encrypted_firmware_file",
			?cause,
			output_file = valuable(&output_path),
			"Could not write encrypted file contents.",
		);

		std::process::exit(ENCRYPT_FW_COULD_NOT_WRITE_FW);
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
		error!(
			id = "bridgectl::mion::encrypt_firmware::conflicting_output_paths",
			flag.output = valuable(&output_path_flag),
			argument.output = valuable(&output_path_positional),
			"Specified the Output Path twice with different values, not sure where to output.",
		);

		std::process::exit(ENCRYPT_FW_BAD_OUTPUT_PATH);
	}

	if let Some(flag) = output_path_flag {
		flag
	} else if let Some(positional) = output_path_positional {
		positional
	} else {
		let mut os_str = firmware_file.as_os_str().to_os_string();
		os_str.push(".encrypted");
		PathBuf::from(os_str)
	}
}
