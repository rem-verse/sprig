//! APIs for interacting with MION Firmware Files.

use crate::errors::APIError;
use aes::{cipher::KeyInit, Aes256};
use cipher::{block_padding::NoPadding, BlockDecryptMut, BlockEncryptMut, BlockSizeUser};
use ecb::{Decryptor, Encryptor};

type Aes256EcbEnc = Encryptor<Aes256>;
type Aes256EcbDec = Decryptor<Aes256>;

/// The derived AES key used for firmware encryption/decryption.
const STOCK_FW_KEY: [u8; 32] = [
	0xA9, 0xFE, 0x4F, 0x78, 0x26, 0x3A, 0xE0, 0xE0, 0xC8, 0xFF, 0x39, 0x95, 0xE4, 0x43, 0x1F, 0x74,
	0x87, 0x9D, 0x1C, 0x67, 0x04, 0x29, 0xBC, 0x79, 0xA5, 0xE3, 0x35, 0x47, 0x8A, 0x60, 0x3B, 0x22,
];

/// A MION Firmware File.
///
/// The MION itself has 3 types of firmware:
///
/// - "fpga.${version}.bin"
/// - "fw.${version}.bin"
/// - "ipl.${version}.bin"
///
/// Each of these are used for separate part of firmware, but all follow the
/// same format. An encrypted blob, followed by a 4 byte version string,
/// followed by a checksum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MIONFirmwareFile {}

// TODO(mythra): what's the checksum'ing algo here???

/// Encrypt a firmware files contents so it can be uploaded.
///
/// ## Errors
///
/// If there is a problem encrypting your data. See error codes
/// from the [`aes`], and [`ecb`] crates.
fn raw_encrypt(file_contents: &[u8]) -> Vec<u8> {
	let encryptor = Aes256EcbEnc::new(&STOCK_FW_KEY.into());
	encryptor.encrypt_padded_vec_mut::<NoPadding>(file_contents)
}

/// Decrypt a firmware files contents so it can be uploaded.
///
/// ## Errors
///
/// If your data is not the correct size to be decrypted.
fn raw_decrypt(file_contents: &[u8]) -> Result<Vec<u8>, APIError> {
	let decryptor = Aes256EcbDec::new(&STOCK_FW_KEY.into());

	decryptor
		.decrypt_padded_vec_mut::<NoPadding>(file_contents)
		.map_err(|_| APIError::BadEncryptedDataLength(Aes256EcbDec::block_size()))
}

#[cfg(test)]
mod unit_tests {
	use super::*;
	use std::path::PathBuf;

	#[must_use]
	pub fn get_test_data_path(relative_to_test_data: &str) -> PathBuf {
		let mut final_path = PathBuf::from(
			std::env::var("CARGO_MANIFEST_DIR")
				.expect("Failed to read `CARGO_MANIFEST_DIR` to locate t est files!"),
		);
		final_path.push("src");
		final_path.push("mion");
		final_path.push("test-data");
		for file_part in relative_to_test_data.split('/') {
			if file_part.is_empty() {
				continue;
			}
			final_path.push(file_part);
		}
		final_path
	}

	#[test]
	pub fn can_decrypt_and_reencrypt_fw() {
		for (source_file_name, dest_file_name) in vec![
			("/fpga.13052071.bin", "/fpga.13052071_d.bin"),
			("/fw.0.00.14.80.bin", "/fw.0.00.14.80_d.bin"),
			("/ipl.0.5.bin", "/ipl.0.5_d.bin"),
		] {
			let encrypted_path = get_test_data_path(source_file_name);
			let decrypted_path = get_test_data_path(dest_file_name);

			let full_encrypted_contents =
				std::fs::read(&encrypted_path).expect("Failed to read encrypted file to decrypt!");
			let decrypted =
				raw_decrypt(&full_encrypted_contents[..]).expect("Failed to decrypt data!");
			let expected_decrypted_contents = std::fs::read(&decrypted_path)
				.expect("Failed to read expected decrypted contents!");

			assert_eq!(
				decrypted.len(),
				expected_decrypted_contents.len(),
				"Decrypted data length did not match expected decrypted data length, file: {}",
				encrypted_path.display(),
			);
			for (idx, byte) in decrypted.iter().enumerate() {
				if *byte != expected_decrypted_contents[idx] {
					panic!(
            "Decrypted Byte at Location: {idx} did not match expected contents! (total: {})",
            decrypted.len(),
          );
				}
			}

			let re_encrypted = raw_encrypt(&decrypted);
			assert_eq!(
				re_encrypted.len(),
				full_encrypted_contents.len(),
				"Encrypted data length did not match expected encrypted data length!",
			);
			for (idx, byte) in re_encrypted.iter().enumerate() {
				if *byte != full_encrypted_contents[idx] {
					panic!(
            "Re-Encrypted Byte at Location: {idx} did not match expected contents! (total: {})",
            re_encrypted.len(),
          );
				}
			}
		}
	}
}
