use std::fmt::{Display, Formatter, Result as FmtResult};

/// MION firmware versions that are listed when performing an HTTP GET request
/// to `/update.cgi`.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct MIONFirmwareVersions {
	/// The three FW bytes of the MION FW.
	fw: [u8; 3],
	/// The version of the FPGA firmware (uses a seperate scheme than the MION
	/// FW).
	fpga: u32,
}
impl MIONFirmwareVersions {
	#[must_use]
	pub const fn from_versions(fw: [u8; 3], fpga: u32) -> Self {
		Self { fw, fpga }
	}

	#[must_use]
	pub const fn get_mion_version(&self) -> [u8; 3] {
		self.fw
	}
	/// Get a human readable version of the MION FW.
	#[must_use]
	pub fn displayable_mion_version(&self) -> String {
		format!("0.{:02}.{}.{}", self.fw[0], self.fw[1], self.fw[2])
	}

	#[must_use]
	pub const fn get_fpga_version(&self) -> u32 {
		self.fpga
	}
	/// Get a human readable display version of the FPGA version.
	#[must_use]
	pub fn displayable_fpga_version(&self) -> String {
		format!("{:02X}", self.fpga)
	}
}
impl Display for MIONFirmwareVersions {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		write!(
			fmt,
			"MION: {} / FPGA: {}",
			self.displayable_mion_version(),
			self.displayable_fpga_version(),
		)
	}
}
