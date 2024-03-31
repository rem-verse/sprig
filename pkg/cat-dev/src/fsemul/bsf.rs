//! Code for creating/reading/managing BOOT1 System Files.
//!
//! `.bsf` files, or BOOT1 System Files as they're called in several places in
//! the original cat-dev sources/documentations are the original files served
//! by PCFS.

use crate::errors::FSError;
use bytes::{BufMut, Bytes, BytesMut};

const MIN_FILE_SIZE: usize = 32_usize;
const MAX_FILE_SIZE: usize = 1_048_576_usize;
const BOOTOS_BOOT_BLOCK_MAGIC: u32 = 0xFD9B_5B7A_u32;

/// A boot file system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootSystemFile {
	header: BootOSBootBlockHeader,
	data_info: Vec<BootOSDataInfo>,
}

impl BootSystemFile {
	#[must_use]
	pub const fn header(&self) -> &BootOSBootBlockHeader {
		&self.header
	}
	#[must_use]
	pub const fn data_info(&self) -> &Vec<BootOSDataInfo> {
		&self.data_info
	}
}

impl Default for BootSystemFile {
	fn default() -> Self {
		Self {
			header: BootOSBootBlockHeader {
				platform_info_physical_address: 0x2000_8000,
				package_info_physical_address: 0x2000_8800,
				boot1_flags: 0x8000_0000,
				sticky_register_bits: 0,
			},
			data_info: Vec::with_capacity(0),
		}
	}
}

impl From<BootSystemFile> for Bytes {
	fn from(value: BootSystemFile) -> Self {
		let mut buff = BytesMut::with_capacity(32 + (value.data_info.len() * 8));

		buff.put_u32(BOOTOS_BOOT_BLOCK_MAGIC);
		// Can't have more infos than this without extending file size.
		let actual_len = std::cmp::min(
			u32::try_from(value.data_info().len()).unwrap_or(131_068),
			131_068,
		);
		buff.put_u32(actual_len);
		buff.put_u32(value.header().platform_info_physical_address());
		buff.put_u32(value.header().package_info_physical_address());
		buff.put_u32(value.header().boot1_flags());
		buff.put_u32(value.header().sticky_register_bits());
		// Padding Bytes
		buff.put_u32(0x0);
		buff.put_u32(0x0);
		for idx in 0..usize::try_from(actual_len).unwrap_or(usize::MAX) {
			buff.put_u32(value.data_info[idx].address());
			buff.put_u32(value.data_info[idx].length());
		}

		buff.freeze()
	}
}

impl TryFrom<Bytes> for BootSystemFile {
	type Error = FSError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		if value.len() < 32 {
			return Err(FSError::TooSmall(MIN_FILE_SIZE, value.len()));
		}
		if value.len() > MAX_FILE_SIZE {
			return Err(FSError::TooLarge(MAX_FILE_SIZE, value.len()));
		}

		let actual_magic = u32::from_be_bytes([value[0], value[1], value[2], value[3]]);
		if actual_magic != BOOTOS_BOOT_BLOCK_MAGIC {
			return Err(FSError::InvalidFileMagic(
				BOOTOS_BOOT_BLOCK_MAGIC,
				actual_magic,
			));
		}

		let data_info_count = u32::from_be_bytes([value[4], value[5], value[6], value[7]]);
		let expected_file_size =
			32_usize + (usize::try_from(data_info_count).unwrap_or(usize::MAX) * 8_usize);
		if value.len() != expected_file_size {
			return Err(FSError::InvalidFileSize(expected_file_size, value.len()));
		}

		let platform_info_addr = u32::from_be_bytes([value[8], value[9], value[10], value[11]]);
		let package_info_addr = u32::from_be_bytes([value[12], value[13], value[14], value[15]]);
		let boot1_flags = u32::from_be_bytes([value[16], value[17], value[18], value[19]]);
		let sticky_register_bits = u32::from_be_bytes([value[20], value[21], value[22], value[23]]);

		// There are two u32's here that are just used for padding here.
		// Then we get into the data info blocks.
		let mut index = 32_usize;
		let mut data_infos =
			Vec::with_capacity(usize::try_from(data_info_count).unwrap_or(usize::MAX));
		for _ in 0..data_info_count {
			let address = u32::from_be_bytes([
				value[index],
				value[index + 1],
				value[index + 2],
				value[index + 3],
			]);
			let length = u32::from_be_bytes([
				value[index + 4],
				value[index + 5],
				value[index + 6],
				value[index + 7],
			]);

			data_infos.push(BootOSDataInfo { address, length });
			index += 8;
		}

		Ok(Self {
			header: BootOSBootBlockHeader {
				platform_info_physical_address: platform_info_addr,
				package_info_physical_address: package_info_addr,
				boot1_flags,
				sticky_register_bits,
			},
			data_info: data_infos,
		})
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootOSBootBlockHeader {
	platform_info_physical_address: u32,
	package_info_physical_address: u32,
	boot1_flags: u32,
	sticky_register_bits: u32,
}
impl BootOSBootBlockHeader {
	#[must_use]
	pub const fn platform_info_physical_address(&self) -> u32 {
		self.platform_info_physical_address
	}
	#[must_use]
	pub const fn package_info_physical_address(&self) -> u32 {
		self.package_info_physical_address
	}
	#[must_use]
	pub const fn boot1_flags(&self) -> u32 {
		self.boot1_flags
	}
	#[must_use]
	pub const fn sticky_register_bits(&self) -> u32 {
		self.sticky_register_bits
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootOSDataInfo {
	address: u32,
	length: u32,
}
impl BootOSDataInfo {
	#[must_use]
	pub const fn address(&self) -> u32 {
		self.address
	}
	#[must_use]
	pub const fn length(&self) -> u32 {
		self.length
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[test]
	pub fn can_parse_boot_system_file() {
		// Taken from a real `ppc.bsf` in generated by real scripts.

		let file_contents = Bytes::from(vec![
			0xFD, 0x9B, 0x5B, 0x7A, 0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x80, 0x00, 0x20, 0x00,
			0x88, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x00, 0x00,
		]);
		let file =
			BootSystemFile::try_from(file_contents).expect("Failed to parse real life ppc.bsf");

		assert!(
			file.data_info().is_empty(),
			"Expected an empty data info list for Boot System File."
		);

		assert_eq!(
			file.header().platform_info_physical_address(),
			0x20008000,
			"Invalid Platform Info Physical Address from Boot System File.",
		);
		assert_eq!(
			file.header().package_info_physical_address(),
			0x20008800,
			"Invalid Package Info Physical Address from Boot System File.",
		);
		assert_eq!(
			file.header().boot1_flags(),
			0x80000000,
			"Invalid Boot1 Flags from Boot System File.",
		);
		assert_eq!(
			file.header().sticky_register_bits(),
			0,
			"Invalid Sticky Register Bits for Boot System File."
		);
	}
}
