//! Code for creating/reading/managing Disk Layout Files.
//!
//! `.dlf` files, or Disk Layout Files as they're called in several places in
//! the original cat-dev sources/documentations are layouts used in several
//! places as a way of encoding addresses to files on the host filesystem.
//!
//! DLF files are not necissarily valid on any other host than the host they
//! were created on. DLF Files are also guaranteed to have UTF-8 paths.

use crate::{
	errors::{CatBridgeError, FSError},
	fsemul::errors::{FSEmulAPIError, FSEmulFSError},
};
use bytes::{Bytes, BytesMut};
use std::{
	collections::BTreeMap,
	path::{Path, PathBuf},
};
use tokio::fs::metadata as get_path_metadata;
use tracing::warn;

/// The maximum address that can be stored in a DLF file.
const MAX_ADDRESS: u128 = 0x000F_FFFF_FFFF_FFFF_FFFF_u128;

/// A disk layout file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskLayoutFile {
	/// A map of addresses to files on this host.
	address_to_path_map: BTreeMap<u128, PathBuf>,
	/// The current address that the layout file ends at.
	current_ending_address: u128,
	/// The major version of this disk layout file.
	///
	/// This assumes we're interpreting the version as
	/// semver-"like" version where you have
	/// `<major>.<minor>`.
	major_version: u8,
	/// The minor version of this disk layout file.
	///
	/// This assumes we're interpreting the version as
	/// semver-"like" version where you have
	/// `<major>.<minor>`.
	minor_version: u8,
}

impl DiskLayoutFile {
	/// Construct a new disk layout file with just an ending address.
	#[must_use]
	pub fn new(max_address: u128) -> Self {
		let mut map = BTreeMap::new();
		map.insert(max_address, PathBuf::new());

		Self {
			address_to_path_map: map,
			current_ending_address: max_address,
			major_version: 1,
			minor_version: 0,
		}
	}

	/// Get the version string that would appear in the DLF file.
	#[must_use]
	pub fn version(&self) -> String {
		format!("v{}.{:02}", self.major_version, self.minor_version)
	}

	/// Get the major version of this disk layout file.
	#[must_use]
	pub const fn major_version(&self) -> u8 {
		self.major_version
	}

	/// Get the minor version of this disk layout file.
	#[must_use]
	pub const fn minor_version(&self) -> u8 {
		self.minor_version
	}

	/// Get the max address on this DLF.
	#[must_use]
	pub const fn max_address(&self) -> u128 {
		self.current_ending_address
	}

	/// Get the current mapping of addresses to paths on the host filesystem.
	#[must_use]
	pub fn address_to_path_map(&self) -> &BTreeMap<u128, PathBuf> {
		&self.address_to_path_map
	}

	/// Get the path at a particular address.
	#[must_use]
	pub async fn get_path_and_offset_for_file(
		&self,
		requested_address: u128,
	) -> Option<(&PathBuf, u64)> {
		// Attempt to find an exact match.
		if let Some(path) = self.address_to_path_map.get(&requested_address) {
			return Some((path, 0));
		}

		// Otherwise try to find an address that might be in the middle of a file.
		let mut last_addr = 0_u128;
		let mut last_path = self
			.address_to_path_map
			.get(&self.max_address())
			.unwrap_or_else(|| unreachable!());
		for (addr, path) in &self.address_to_path_map {
			if *addr < requested_address {
				last_addr = *addr;
				last_path = path;
				continue;
			}
			let metadata = match get_path_metadata(last_path).await {
				Ok(md) => md,
				Err(cause) => {
					warn!(
						?cause,
						path = %last_path.display(),
						"Failed to get metadata for path, not sure if matching over SDIO, treating as non-match.",
					);
					break;
				}
			};
			let offset = u64::try_from(requested_address - last_addr).unwrap_or(u64::MAX);
			// Whee! We did find a match, and it's right in the middle of another
			// file.
			if metadata.len() > offset {
				return Some((last_path, offset));
			}

			// Break if we can't be in the middle of a file.
			break;
		}

		// No match found.
		None
	}

	/// Insert, or update the path at a particular address.
	///
	/// ## Errors
	///
	/// - If you try to write an address past [`MAX_ADDRESS`].
	/// - If you try inserting an address past the ending of PATH.
	/// - If we cannot canonicalize the path you past in.
	pub fn upsert_addressed_path(
		&mut self,
		address: u128,
		path: &Path,
	) -> Result<(), CatBridgeError> {
		if address >= MAX_ADDRESS {
			return Err(FSEmulAPIError::DlfAddressTooLarge(address, MAX_ADDRESS).into());
		}

		let mut update_ending_address = false;
		// If we're trying to write past the end, and not just update the end.
		if address > self.current_ending_address {
			if !path.as_os_str().is_empty() {
				return Err(FSEmulAPIError::DlfUpsertEndingFirst.into());
			}

			update_ending_address = true;
		}

		let canonicalized_path = path.canonicalize().map_err(FSError::from)?;
		// Validate our canonicalized path is UTF-8.
		let Ok(_) = canonicalized_path.as_os_str().to_owned().into_string() else {
			return Err(FSEmulAPIError::DlfPathMustBeUtf8(Bytes::from(Vec::from(
				canonicalized_path.as_os_str().to_owned().as_encoded_bytes(),
			)))
			.into());
		};
		self.address_to_path_map.insert(address, canonicalized_path);

		if update_ending_address {
			self.current_ending_address = address;
		}

		Ok(())
	}

	/// Remove a path at a particular address.
	///
	/// ## Errors
	///
	/// If you try to remove the ending address for the DLF file. DLF files do
	/// require an ending.
	pub fn remove_path_at_address(&mut self, address: u128) -> Result<(), FSEmulAPIError> {
		if address == self.current_ending_address {
			return Err(FSEmulAPIError::DlfMustHaveEnding);
		}
		self.address_to_path_map.remove(&address);

		Ok(())
	}
}

impl From<&DiskLayoutFile> for Bytes {
	fn from(value: &DiskLayoutFile) -> Self {
		let mut bytes = BytesMut::new();
		bytes.extend_from_slice(value.version().as_bytes());
		bytes.extend_from_slice(b"\r\n");
		for (address, path) in &value.address_to_path_map {
			bytes.extend_from_slice(
				format!(
					"0x{address:016X},\"{}\"\r\n",
					// We already know it's utf-8, so we know this is safe.
					path.to_string_lossy(),
				)
				.as_bytes(),
			);
		}

		bytes.freeze()
	}
}

impl From<DiskLayoutFile> for Bytes {
	fn from(value: DiskLayoutFile) -> Self {
		Self::from(&value)
	}
}

impl TryFrom<Bytes> for DiskLayoutFile {
	type Error = FSError;

	fn try_from(value: Bytes) -> Result<Self, Self::Error> {
		let as_utf8 = String::from_utf8(value.to_vec())?;
		let lines = as_utf8.split("\r\n").collect::<Vec<_>>();
		// We expect at least 3 lines!
		//
		// ```text
		// <version line>
		// <end of dlf>
		//
		// ```
		//
		// (that last blank line is just because files are guaranteed to end with
		// `\r\n`).
		if lines.len() < 3 {
			return Err(FSError::TooFewLines(lines.len(), 3_usize));
		}

		let mut address_map = BTreeMap::new();
		// For each line that maps to an address.
		//
		// EXCEPT the final line which should always be our empty address.
		let mut last_read_address: u128 = 0;
		for line in &lines[1..lines.len() - 2] {
			let mut iterator = line.splitn(2, ',');
			let Some(address_str) = iterator.next() else {
				return Err(FSEmulFSError::DlfCorruptLine((*line).to_owned()).into());
			};
			let Some(path_string) = iterator.next() else {
				return Err(FSEmulFSError::DlfCorruptLine((*line).to_owned()).into());
			};

			// Parse out an address, it should be in increasing order...
			let address = u128::from_str_radix(address_str.trim_start_matches("0x"), 16)
				.map_err(|_| FSEmulFSError::DlfCorruptLine((*line).to_owned()))?;
			// For the first address they can be equal... for anything past the start
			// we can't have two files in the same place.
			if last_read_address != 0 && address <= last_read_address {
				return Err(FSEmulFSError::DlfCorruptLine((*line).to_owned()).into());
			}
			last_read_address = address;

			// Parse out the path! We don't necissarily need to check it exists,
			// just that it's absolute...
			let path = PathBuf::from(path_string.trim_matches('"'));
			// Empty path aka the end is only available at the last line
			// which we explicitly exclude from the loop.
			if path.as_os_str().is_empty() {
				return Err(FSEmulFSError::DlfCorruptLine((*line).to_owned()).into());
			}
			if !path.is_absolute() {
				return Err(FSEmulFSError::DlfCorruptLine((*line).to_owned()).into());
			}
			address_map.insert(address, path);
		}

		let should_be_ending_line = lines[lines.len() - 2];
		let mut ending_iter = should_be_ending_line.splitn(2, ',');
		let Some(final_address_str) = ending_iter.next() else {
			return Err(FSEmulFSError::DlfCorruptLine(should_be_ending_line.to_owned()).into());
		};
		let Some(final_path_str) = ending_iter.next() else {
			return Err(FSEmulFSError::DlfCorruptLine(should_be_ending_line.to_owned()).into());
		};
		let final_address = u128::from_str_radix(final_address_str.trim_start_matches("0x"), 16)
			.map_err(|_| FSEmulFSError::DlfCorruptLine(should_be_ending_line.to_owned()))?;

		if last_read_address != 0 && final_address <= last_read_address {
			return Err(FSEmulFSError::DlfCorruptLine(should_be_ending_line.to_owned()).into());
		}
		if final_path_str != r#""""# {
			return Err(FSEmulFSError::DlfCorruptLine(should_be_ending_line.to_owned()).into());
		}
		address_map.insert(final_address, PathBuf::new());
		if !lines[lines.len() - 1].is_empty() {
			return Err(
				FSEmulFSError::DlfCorruptFinalLine(lines[lines.len() - 1].to_owned()).into(),
			);
		}

		let version_str = lines[0];
		if !version_str.starts_with('v') {
			return Err(FSEmulFSError::DlfCorruptVersionLine(version_str.to_owned()).into());
		}
		let mut version_iter = version_str.trim_start_matches('v').splitn(2, '.');
		let Some(major_version_str) = version_iter.next() else {
			return Err(FSEmulFSError::DlfCorruptVersionLine(version_str.to_owned()).into());
		};
		let Some(minor_version_str) = version_iter.next() else {
			return Err(FSEmulFSError::DlfCorruptVersionLine(version_str.to_owned()).into());
		};
		let Ok(major_version) = major_version_str.parse::<u8>() else {
			return Err(FSEmulFSError::DlfCorruptVersionLine(version_str.to_owned()).into());
		};
		let Ok(minor_version) = minor_version_str.parse::<u8>() else {
			return Err(FSEmulFSError::DlfCorruptVersionLine(version_str.to_owned()).into());
		};

		Ok(Self {
			address_to_path_map: address_map,
			current_ending_address: final_address,
			major_version,
			minor_version,
		})
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[must_use]
	pub fn get_test_data_path(relative_to_test_data: &str) -> PathBuf {
		let mut final_path = PathBuf::from(
			std::env::var("CARGO_MANIFEST_DIR")
				.expect("Failed to read `CARGO_MANIFEST_DIR` to locate test files!"),
		);
		final_path.push("src");
		final_path.push("fsemul");
		final_path.push("test-data");
		for file_part in relative_to_test_data.split('/') {
			if file_part.is_empty() {
				continue;
			}
			final_path.push(file_part);
		}
		final_path
	}

	#[tokio::test]
	pub async fn can_parse_real_files() {
		// Just validate these don't error.
		let real_life_dlf = Bytes::from(
			std::fs::read(get_test_data_path("ppc_boot.dlf"))
				.expect("Failed to read `ppc_boot.dlf` test data file!"),
		);
		let empty_dlf = Bytes::from(
			std::fs::read(get_test_data_path("minimal.dlf"))
				.expect("Failed to read `minimal.dlf` test data file!"),
		);

		let dlf = DiskLayoutFile::try_from(real_life_dlf.clone())
			.expect("Failed to parse real life dlf file!");
		let edlf = DiskLayoutFile::try_from(empty_dlf.clone())
			.expect("Failed to parse the most minimal of disk layout files!");

		assert_eq!(
			dlf.major_version(),
			1,
			"Real-DLF didnt parse correct major version!"
		);
		assert_eq!(
			dlf.minor_version(),
			0,
			"Real-DLF didn't parse correct minor version!"
		);
		assert_eq!(
			dlf.get_path_and_offset_for_file(0x80000_u128).await,
			Some((
				&PathBuf::from(r#"/opt/cafe_sdk/temp/mythra/caferun/ppc.bsf"#),
				0
			)),
			"Real-DLF did not match correct path for address.",
		);
		assert_eq!(
			Bytes::from(dlf),
			real_life_dlf,
			"Failed to serialize real life DLF into the exact same contents as real life DLF!"
		);

		assert_eq!(
			edlf.major_version(),
			1,
			"Empty DLF didn't parse correct major version!"
		);
		assert_eq!(
			edlf.minor_version(),
			0,
			"Empty DLF didn't parse correct minor version!"
		);
		assert_eq!(
			edlf.get_path_and_offset_for_file(0x0_u128).await,
			Some((&PathBuf::new(), 0)),
			"Empty dlf did not match correct path for address.",
		);
		assert_eq!(
			Bytes::from(edlf),
			empty_dlf,
			"Failed to serialize empty DLF into the exact same contents as empty DLF!"
		);
	}
}
