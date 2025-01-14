//! APIs for interacting with MION Firmware Files.

use aes::{cipher::KeyInit, Aes256};
use cipher::{block_padding::NoPadding, BlockDecryptMut, BlockEncryptMut, BlockSizeUser};
use ecb::{Decryptor, Encryptor};
use miette::Diagnostic;
use std::fmt::{Display, Formatter, Result as FmtResult};
use thiserror::Error;

type Aes256EcbEnc = Encryptor<Aes256>;
type Aes256EcbDec = Decryptor<Aes256>;

/// The derived AES key used for firmware encryption/decryption.
const STOCK_FW_KEY: [u8; 32] = [
	0xA9, 0xFE, 0x4F, 0x78, 0x26, 0x3A, 0xE0, 0xE0, 0xC8, 0xFF, 0x39, 0x95, 0xE4, 0x43, 0x1F, 0x74,
	0x87, 0x9D, 0x1C, 0x67, 0x04, 0x29, 0xBC, 0x79, 0xA5, 0xE3, 0x35, 0x47, 0x8A, 0x60, 0x3B, 0x22,
];

/// The MION actually contains multiple types of firmware, this enum encodes
/// those types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MIONFirmwareType {
	/// The MION has an FPGA array, these firmware files target those.
	Fpga,
	/// The MION has an IPL chip similar to the gamecube, these firmware files
	/// target that chip.
	Ipl,
	/// The root firmware for the main MION board.
	Mion,
}
impl Display for MIONFirmwareType {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		match *self {
			Self::Fpga => write!(fmt, "fpga"),
			Self::Ipl => write!(fmt, "ipl"),
			Self::Mion => write!(fmt, "fw"),
		}
	}
}

/// A MION Firmware File.
///
/// The MION itself has 3 types of firmware:
///
/// - "fpga.${version}.bin" aka [`MIONFirmwareType::Fpga`]
/// - "fw.${version}.bin" aka [`MIONFirmwareType::Ipl`]
/// - "ipl.${version}.bin" aka [`MIONFirmwareType::Mion`]
///
/// Each of these are used for separate part of firmware, but all follow the
/// same format. An encrypted blob, followed by a 4 byte version string,
/// followed by a checksum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MIONFirmwareFile {
	/// The actual firmware contents, this is *decrypted* when represented in
	/// memory here.
	data: Vec<u8>,
	/// The type of firmware this is.
	fw_type: MIONFirmwareType,
	/// The 4 bytes that represent versions.
	///
	/// Each firmware type treats these bytes _slightly_
	/// differently. Thanks Nintendo.
	version_bytes: [u8; 4],
	/// I have no clue what this byte is.
	unk_byte: u8,
	/// The checksum byte of everything else in the file. Very last byte.
	checksum: u8,
}

impl MIONFirmwareFile {
	/// Attempt to parse a firmware file that would be uploaded to the MION
	/// board.
	///
	/// These packages would be manually uploaded to:
	/// `http://<mionip>/update.cgi`.
	///
	/// ## Errors
	///
	/// This function will error if the original MION firmwares would throw an
	/// error at this package, or would otherwise corrupt when installing it.
	/// This consistetues the following checks:
	///
	/// - Validating the file is at least 0x26 (38) bytes long
	///   (*note: the mion only checks for 0x16 but will fail because the AES
	///   block size is 32 bytes.*)
	/// - The file has an invalid checksum (the checksum is the last byte in
	///   the file.)
	/// - If uploading a firmware type of [`MIONFirmwareType::Mion`], the last
	///   byte in the version must be 0x00, if it's not the upload will corrupt
	///   heavily as the programmers treated it as a load-bearing NUL terminator
	///   which is required for strings in C (the language the FW was written
	///   in).
	/// - If the main contents of the file were not encrypted with the common AES
	///   key.
	/// - If the decrypted contents do not end with the correct signature, this is
	///   `PWI-SS_FW_IMAGE` for [`MionFirmwareType::Mion`] &
	///   [`MionFirmwareType::Ipl`], and `PWI-SS_FP_IMAGE` for
	///   [`MionFirmwareType::FPGA`].
	pub fn parse(
		firmware: &[u8],
		firmware_type: MIONFirmwareType,
	) -> Result<Self, MIONFirmwareAPIError> {
		let firmware_length = firmware.len();
		if firmware_length < 0x26 {
			return Err(MIONFirmwareAPIError::TooSmall(firmware_length));
		}
		let chksum_in_file = firmware[firmware_length - 1];
		let got_chksum = calculate_checksum(firmware);
		if chksum_in_file != got_chksum {
			return Err(MIONFirmwareAPIError::BadChecksum(
				chksum_in_file,
				got_chksum,
			));
		}
		// This check is uniquely ours, the MION does not have this.
		//
		// But this really fucks up a lot, because it's a load bearing NUL
		// terminator.
		if matches!(firmware_type, MIONFirmwareType::Mion) && firmware[firmware_length - 3] != 0x00
		{
			return Err(MIONFirmwareAPIError::MissingNULTerminator(
				firmware[firmware_length - 3],
			));
		}
		let decrypted = raw_decrypt(&firmware[..firmware_length - 6])?;

		let expected_footer = match firmware_type {
			MIONFirmwareType::Fpga => b"PWI-SS_FP_IMAGE\0",
			_ => b"PWI-SS_FW_IMAGE\0",
		};
		if !decrypted.ends_with(expected_footer) {
			return Err(MIONFirmwareAPIError::MissingSignature);
		}

		Ok(Self {
			data: decrypted,
			fw_type: firmware_type,
			version_bytes: [
				firmware[firmware_length - 6],
				firmware[firmware_length - 5],
				firmware[firmware_length - 4],
				firmware[firmware_length - 3],
			],
			unk_byte: firmware[firmware_length - 2],
			checksum: chksum_in_file,
		})
	}

	/// Get the checksum for this firmware file.
	#[must_use]
	pub const fn checksum(&self) -> u8 {
		self.checksum
	}

	/// Get the underlying decrypted contents of this firmware file.
	#[must_use]
	pub const fn contents(&self) -> &Vec<u8> {
		&self.data
	}

	/// Get the type of firmware this is.
	#[must_use]
	pub const fn firmware_type(&self) -> MIONFirmwareType {
		MIONFirmwareType::Mion
	}

	/// Get the bytes that would need to be written to a file, and uploaded to
	/// `/update.cgi` on the MION to deploy this firmware file.
	#[allow(
		// This function can't actually panic, it's just raw_encrypt doesn't know
		// we've pre-validated the length by decrypting it successfully and not
		// offering mutable APIs.
		clippy::missing_panics_doc,
	)]
	#[must_use]
	pub fn get_deployable_firmware_data(&self) -> Vec<u8> {
		let mut encrypted =
			raw_encrypt(&self.data).expect("We validate the block size at parse time.");
		encrypted.extend_from_slice(&self.version_bytes);
		encrypted.push(self.unk_byte);
		encrypted.push(self.checksum);
		encrypted
	}

	/// Get the version for the actual firmware file.
	///
	/// Each firmware type has a slightly different format than the rest of the
	/// firmware types. An example of known following firmware types are:
	///
	/// - [`MIONFirmwareType::Mion`] -> `0.00.14.80`
	/// - [`MIONFirmwareType::Fpga`] -> `13052071`
	/// - [`MIONFirmwareType::Ipl`] -> `0.5`
	#[must_use]
	pub fn version(&self) -> String {
		match self.fw_type {
			MIONFirmwareType::Mion => format!(
				"0.{:02}.{}.{}",
				self.version_bytes[0], self.version_bytes[1], self.version_bytes[2],
			),
			MIONFirmwareType::Fpga => format!(
				"{:02X}{:02X}{:02X}{:02X}",
				self.version_bytes[3],
				self.version_bytes[2],
				self.version_bytes[1],
				self.version_bytes[0],
			),
			MIONFirmwareType::Ipl => format!(
				"{}.{}",
				u16::from_le_bytes([self.version_bytes[0], self.version_bytes[1]]),
				u16::from_le_bytes([self.version_bytes[2], self.version_bytes[3]]),
			),
		}
	}
}

/// Errors related to handling mion firmwares.
#[derive(Error, Diagnostic, Debug, PartialEq, Eq)]
pub enum MIONFirmwareAPIError {
	/// You attempted to encrypt data that we could not encrypt.
	#[error("We could not encrypt your data, because it was not padded to the correct length, expected a block size of: {0}")]
	#[diagnostic(code(cat_dev::api::mion::firmware::bad_decrypted_data_length))]
	BadDecryptedDataLength(usize),
	/// You attempted to decrypt data that we could not decrypt.
	#[error("We could not decrypt your data, because it was not padded to the correct length, expected a block size of: {0}")]
	#[diagnostic(code(cat_dev::api::mion::firmware::bad_encrypted_data_length))]
	BadEncryptedDataLength(usize),
	/// The MION Firmware files end with a final byte that acts as a checksum
	/// to validate the content before it was correct. Your checksum was not
	/// correct.
	#[error("The MION Firmware file you provided had an invalid checksum, we expected: {1:02x}, but got: {0:02x}")]
	#[diagnostic(code(cat_dev::api::mion::firmware::bad_checksum))]
	BadChecksum(u8, u8),
	/// All MION Firmware files must be at a minimum 0x26 bytes long.
	///
	/// This covers a single AES-256 block (32 bytes), plus the 6 byte footer
	/// that they contain.
	#[error("The MION Firmware file provided was too small, it must be at least 0x26 bytes long, was {0:02x}")]
	#[diagnostic(code(cat_dev::api::mion::firmware::too_small))]
	TooSmall(usize),
	/// The MION Firmware version string must end with a `0x00`, as this is a
	/// load-bearing NUL terminator for many parts of the firmware.
	#[error("The Version String for MION Firmware Files Typed 'MION', must have their version bytes end with a NUL terminator (0x00) due to an oversight in programming. Your file ended with: ({0:02x})")]
	#[diagnostic(code(cat_dev::api::mion::firmware::missing_nul_terminator))]
	MissingNULTerminator(u8),
	/// All MION FW files must end with:
	///
	/// - `PWI-SS_FW_IMAGE` for IPL/MION firmware types
	/// - `PWI-SS_FP_IMAGE` for FPGA firmware types.
	///
	/// If they do not, they are immediately considered invalid.
	#[error("While validating the decrypted contents of your FW we were not able to identify the required ending bytes, this firmware is corrupt.")]
	#[diagnostic(code(cat_dev::api::mion_fw::missing_signature))]
	MissingSignature,
}

/// Calculate a checksum for a given _encrypted_ blob.
///
/// The checksum present as the last byte in the file takes in all the content
/// before the final byte of the encrypted file, and spits out a single `u8`
/// that represents the final checksum of the firmware.
///
/// ## Panics
///
/// This function should never panic, however there is an `expect` incase math
/// ever fundamenetally breaks and performing a `& 0xFF` returns us _more_ than
/// the last 8 bits.
fn calculate_checksum(encrypted_blob: &[u8]) -> u8 {
	let mut chksum = 0_u32;
	// For every byte except the last byte (CHKSUM), use it's value to calculate
	// the checksum.
	for byte in encrypted_blob.iter().take(encrypted_blob.len() - 1) {
		chksum = chksum.wrapping_add((*byte).into());
	}
	while chksum & 0xFFFF_FF00_u32 != 0 {
		chksum = (chksum & 0xFF) + (chksum >> 8);
	}
	u8::try_from(!chksum & 0xFF)
		.expect("&0xFF did not just give us the last 8 bits??? is math broken?")
}

/// Encrypt a firmware files contents so it can be uploaded.
///
/// ## Errors
///
/// If there is a problem encrypting your data. See error codes
/// from the [`aes`], and [`ecb`] crates.
#[doc(hidden)]
pub fn raw_encrypt(file_contents: &[u8]) -> Result<Vec<u8>, MIONFirmwareAPIError> {
	let encryptor = Aes256EcbEnc::new(&STOCK_FW_KEY.into());
	let mut decrypted = vec![
		0x0;
		Aes256EcbEnc::block_size()
			* (file_contents.len() / Aes256EcbEnc::block_size() + 1)
	];
	let actual_len = encryptor
		.encrypt_padded_b2b_mut::<NoPadding>(file_contents, &mut decrypted)
		.map_err(|_| MIONFirmwareAPIError::BadDecryptedDataLength(Aes256EcbEnc::block_size()))?
		.len();
	decrypted.truncate(actual_len);
	Ok(decrypted)
}

/// Decrypt a firmware files contents so it can be uploaded.
///
/// ## Errors
///
/// If your data is not the correct size to be decrypted.
#[doc(hidden)]
pub fn raw_decrypt(file_contents: &[u8]) -> Result<Vec<u8>, MIONFirmwareAPIError> {
	let decryptor = Aes256EcbDec::new(&STOCK_FW_KEY.into());

	decryptor
		.decrypt_padded_vec_mut::<NoPadding>(file_contents)
		.map_err(|_| MIONFirmwareAPIError::BadEncryptedDataLength(Aes256EcbDec::block_size()))
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

			let re_encrypted = raw_encrypt(&decrypted).expect("Failed to encrypt firmware!");
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

	#[test]
	pub fn correctly_calculates_checksums() {
		// These test constants were all taken from real life MION files.
		//
		// You can see what the MION would "expect" by connecting to a running
		// MION device with telnet (the username is `mion`, and password is
		// `/Multi_I/O_Network/`, the same as the HTTP interface.) Then run the
		// following commands:
		//
		// ```bash
		// INET> enable_telnet_trace
		// Telnet port is ENABLED to transmit debug logs...
		//
		// INET> enable_debug SATA_H|SATA_D|FPGA|PCIE|EEPROM|HTTP|FWUPD|FPUPD|BIOS|KERNEL
		// enable_debug SATA_H|SATA_D|FPGA|PCIE|EEPROM|HTTP|FWUPD|FPUPD|BIOS|KERNEL modules are enabled to produce debug logs...
		// ```
		//
		// Then upload a firmware file with a purposefully bad checksum (the last
		// byte in the file). And in your existing telnet session run:
		//
		// ```bash
		// INET> task_trace
		// [...snipped...]
		// 9, TS - 191574, Msg - *** ERR: [HTTP]  >Checck sum : Calc, Image 21, 20
		//
		// 9, TS - 193977, Msg - === INF: [HTTP]  >_http_update_fw_html Enter, seq = -2, Mode = Firmware
		// ```
		//
		// The numbers will most likely be different at the beginning but what we
		// care about is the: `Checck sum : Calc, Image 21, 20` line. In this case
		// it's letting us know it calculated a checksum of 0x21, but the file had
		// a checksum of 0x20. So it would have showed the error page.
		//
		// You can use this to validate the output of any file.

		for (file_path, footer_data, name, expected_value) in vec![
			(
				"/fw.0.00.14.80.bin",
				vec![0x00_u8, 0x0E, 0x50, 0x00, 0xA1, 0x00],
				"$encrypted.0.14.80.0.$0xA1",
				0x21_u8,
			),
			(
				"/fw.0.00.14.80.bin",
				vec![0x01_u8, 0x0E, 0x50, 0x00, 0xA1, 0x00],
				"$encrypted.1.14.80.0.$0xA1",
				0x20_u8,
			),
			(
				"/fw.0.00.14.80.bin",
				vec![0xFF_u8, 0x0E, 0x50, 0x00, 0xA1, 0x00],
				"$encrypted.255.14.80.0.$0xA1",
				0x21_u8,
			),
			(
				"/fw.0.00.14.80.bin",
				vec![0xCF_u8, 0x0E, 0x50, 0x00, 0xA1, 0x00],
				"$encrypted.207.14.80.0.$0xA1",
				0x51_u8,
			),
		] {
			let mut encrypted_contents = std::fs::read(get_test_data_path(file_path))
				.expect("Failed to read test data file!");
			encrypted_contents.extend(footer_data);
			assert_eq!(
				calculate_checksum(&encrypted_contents),
				expected_value,
				"Checksum did not match for named contents: {name}! Please check checksum code!",
			);
		}
	}

	#[test]
	pub fn can_successfully_parse_real_fw_file() {
		let mut mion_fw = std::fs::read(get_test_data_path("/fw.0.00.14.80.bin"))
			.expect("Failed to read encrypted MION FW!");
		let mut ipl_fw = std::fs::read(get_test_data_path("/ipl.0.5.bin"))
			.expect("Failed to read encrypted IPL FW!");
		let mut fpga_fw = std::fs::read(get_test_data_path("/fpga.13052071.bin"))
			.expect("Failed to read encrypted FPGA FW!");
		// Append the real life footers that would've been on these files.
		mion_fw.extend([0x00, 0x0E, 0x50, 0x00, 0xA1, 0x21]);
		ipl_fw.extend([0x00, 0x00, 0x05, 0x00, 0xA1, 0x3E]);
		fpga_fw.extend([0x71, 0x20, 0x05, 0x13, 0xA2, 0xBF]);

		let parsed_mion = MIONFirmwareFile::parse(&mion_fw, MIONFirmwareType::Mion)
			.expect("Failed to parse MION firmware!");
		let parsed_ipl = MIONFirmwareFile::parse(&ipl_fw, MIONFirmwareType::Ipl)
			.expect("Failed to parse IPL firmware!");
		let parsed_fpga = MIONFirmwareFile::parse(&fpga_fw, MIONFirmwareType::Fpga)
			.expect("Failed to parse FPGA firmware!");

		assert_eq!(parsed_mion.version(), "0.00.14.80");
		assert_eq!(parsed_ipl.version(), "0.5");
		assert_eq!(parsed_fpga.version(), "13052071");
	}
}
