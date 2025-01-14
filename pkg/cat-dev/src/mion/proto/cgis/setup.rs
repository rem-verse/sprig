use crate::mion::cgis::MIONCGIApiError;
use mac_address::MacAddress;
use std::{
	fmt::{Display, Formatter, Result as FmtResult},
	net::Ipv4Addr,
};

/// The parameters you can fetch from the `setup.cgi` page.
///
/// While this page is normally really more intended for humans, rather
/// than an automated parsers. HOWEVER, `fsemul` & several other useful
/// functions are only available on this page.
///
/// Why they're not available in the parameter space, or why there's not
/// a `get_param` action, but is a `set_param` action I'll never know.
/// It will make me sad though.
#[allow(
	// Clippy this is not a state machine, >:(
	clippy::struct_excessive_bools,
)]
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct SetupParameters {
	/// The 'ip address' of the bridge, default value is "192.168.0.1".
	///
	/// *note: this value doesn't get used when DHCP is on)*.
	///
	/// Internal HTML ID: `id_5`
	static_ip_address: Ipv4Addr,
	/// The Subnet mask of the bridge, default value is "255.255.255.0".
	///
	/// Internal HTML ID: `id_6`
	subnet_mask: Ipv4Addr,
	/// The default Gateway to reach out to for the bridge, default value is
	/// "0.0.0.0".
	///
	/// Internal HTML ID: `id_7`
	default_gateway: Ipv4Addr,
	/// If the device should be using DHCP, default is TRUE.
	///
	/// Internal HTML ID: `id_4`
	dhcp: bool,
	/// If the device should query it's own DNS Servers, default is FALSE.
	///
	/// Internal HTML ID: `id_8`
	dns: bool,
	/// The Primary DNS Server to reach out too if DNS is flipped on, default is
	/// "0.0.0.0".
	///
	/// Internal HTML ID: `id_9`
	primary_dns_server: Ipv4Addr,
	/// The Secondary DNS Server to reach out too if DNS is flipped on, default is
	/// "0.0.0.0".
	///
	/// Internal HTML ID: `id_10`.
	secondary_dns_server: Ipv4Addr,
	/// If we support jumbo frames on the network, default is TRUE.
	///
	/// Internal HTML ID: `id_11`
	jumbo_frame: bool,
	/// The IP Address of the Host PC to reach out too, default is "0.0.0.0".
	///
	/// Internal HTML ID: `id_12`
	host_pc_ip: Ipv4Addr,
	/// The bank size (or hard-disk) size of the CAT-DEV.
	///
	/// Internal HTML ID is: `id_26`
	bank_size: CatDevBankSize,
	/// The Hard Disk "bank" number to use (which area to read from when booting
	/// a game), default is "0".
	///
	/// Internal HTML ID: `id_27`
	hdd_bank_no: u8,
	/// The port of the ATAPI Emulator, default is `7974`.
	///
	/// Internal HTML ID is: `id_13`
	atapi_emulator_port: u16,
	/// The port of the SDIO Printf/Control traffic, default is `7975`.
	///
	/// Internal HTML ID: `id_14`
	sdio_printf_port: u16,
	/// The port of the SDIO Block Data traffic, default is `7976`.
	///
	/// Internal HTML ID: `id_15`
	sdio_block_port: u16,
	/// The port of the EXI traffic, default is `7977`.
	///
	/// Internal HTML ID: `id_16`
	exi_port: u16,
	/// The port for parameter-space traffic, default is `7978`.
	///
	/// *note: changing this will break official nintendo tools. Although our
	/// tools do uspport it, it will require manually passing the port in. it
	/// is unideal to change this port.*
	///
	/// Internal HTML ID: `id_17`
	parameter_space_port: u16,
	/// If drive timing emulation is on (make the disk slower on purpose),
	/// default is FALSE.
	///
	/// Internal HTML ID: `id_33`
	drive_timing_emulation: bool,
	/// If the CAT-DEV device is operating in CAT-DEV mode, or H-READER mode,
	/// default is FALSE.
	///
	/// Internal HTML ID: `id_32`
	operational_mode_is_reader: bool,
	/// Internal Hard-Drive product revision in hex.
	///
	/// Internal HTML ID: `id_28`
	drive_product_revision: String,
	/// Internal Hard-Drive vendor code in hex.
	///
	/// Internal HTML ID: `id_29`
	drive_vendor_code: String,
	/// Internal Hard-Drive device code in hex.
	///
	/// Internal HTML ID: `id_30`
	drive_device_code: String,
	/// Internal Hard-Drive release date in hex.
	///
	/// Internal HTML ID: `id_31`
	drive_release_date: String,
	/// The unique name of the cat-dev on the network.
	///
	/// Internal HTML ID: `id_2`
	device_name: String,
	/// The MAC Address of the machine.
	///
	/// Internal HTML ID: N/A
	mac_address: MacAddress,
}

impl SetupParameters {
	/// Create a new setup set of parameters.
	#[allow(
		// This is how many fields we have clippy.
		clippy::too_many_arguments,
		// This isn't a state machine, or anything.
		clippy::fn_params_excessive_bools,
	)]
	#[must_use]
	pub const fn new(
		static_ip_address: Ipv4Addr,
		subnet_mask: Ipv4Addr,
		default_gateway: Ipv4Addr,
		dhcp: bool,
		dns: bool,
		primary_dns_server: Ipv4Addr,
		secondary_dns_server: Ipv4Addr,
		jumbo_frame: bool,
		host_pc_ip: Ipv4Addr,
		bank_size: CatDevBankSize,
		hdd_bank_no: u8,
		atapi_emulator_port: u16,
		sdio_printf_port: u16,
		sdio_block_port: u16,
		exi_port: u16,
		parameter_space_port: u16,
		drive_timing_emulation: bool,
		operational_mode_is_reader: bool,
		drive_product_revision: String,
		drive_vendor_code: String,
		drive_device_code: String,
		drive_release_date: String,
		device_name: String,
		mac_address: MacAddress,
	) -> Self {
		Self {
			static_ip_address,
			subnet_mask,
			default_gateway,
			dhcp,
			dns,
			primary_dns_server,
			secondary_dns_server,
			jumbo_frame,
			host_pc_ip,
			bank_size,
			hdd_bank_no,
			atapi_emulator_port,
			sdio_printf_port,
			sdio_block_port,
			exi_port,
			parameter_space_port,
			drive_timing_emulation,
			operational_mode_is_reader,
			drive_product_revision,
			drive_vendor_code,
			drive_device_code,
			drive_release_date,
			device_name,
			mac_address,
		}
	}

	/// Create a new set of default settings based on the device name, and mac.
	#[must_use]
	pub fn default_settings(device_name: String, mac_address: MacAddress) -> Self {
		Self {
			static_ip_address: Ipv4Addr::new(192, 168, 0, 1),
			subnet_mask: Ipv4Addr::new(255, 255, 255, 0),
			default_gateway: Ipv4Addr::new(0, 0, 0, 0),
			dhcp: true,
			dns: false,
			primary_dns_server: Ipv4Addr::new(0, 0, 0, 0),
			secondary_dns_server: Ipv4Addr::new(0, 0, 0, 0),
			jumbo_frame: true,
			host_pc_ip: Ipv4Addr::new(0, 0, 0, 0),
			bank_size: CatDevBankSize::Blank,
			hdd_bank_no: 0,
			atapi_emulator_port: 7974,
			sdio_printf_port: 7975,
			sdio_block_port: 7976,
			exi_port: 7977,
			parameter_space_port: 7978,
			drive_timing_emulation: false,
			operational_mode_is_reader: false,
			drive_product_revision: String::with_capacity(0),
			drive_vendor_code: String::with_capacity(0),
			drive_device_code: String::with_capacity(0),
			drive_release_date: String::with_capacity(0),
			device_name,
			mac_address,
		}
	}

	/// Get the static ip address being used.
	///
	/// This will return `None` if the console is using DHCP.
	#[must_use]
	pub const fn static_ip_address(&self) -> Option<Ipv4Addr> {
		if self.dhcp {
			None
		} else {
			Some(self.static_ip_address)
		}
	}
	/// Get the value for the static ip address.
	///
	/// *note: may not be correct, and used if dhcp is on.*
	#[must_use]
	pub const fn raw_static_ip_address(&self) -> Ipv4Addr {
		self.static_ip_address
	}

	/// Get the subnet mask for the CAT-DEV device.
	#[must_use]
	pub const fn subnet_mask(&self) -> Ipv4Addr {
		self.subnet_mask
	}

	/// Get the gateway to use for the CAT-DEV.
	#[must_use]
	pub const fn default_gateway(&self) -> Ipv4Addr {
		self.default_gateway
	}

	/// If we're using DHCP to get an IP Address.
	#[must_use]
	pub const fn using_dhcp(&self) -> bool {
		self.dhcp
	}

	/// If we're using our own DNS ips set in our configuration.
	#[must_use]
	pub const fn using_self_managed_dns(&self) -> bool {
		self.dns
	}
	/// Get the primary dns server the cat-dev will be using.
	///
	/// *note: this will return none if the cat-dev isn't using self managed
	/// dns.*
	#[must_use]
	pub const fn primary_dns(&self) -> Option<Ipv4Addr> {
		if self.dns {
			Some(self.primary_dns_server)
		} else {
			None
		}
	}
	/// Get the raw value of the Primary DNS Server for the cat-dev.
	///
	/// *note: this value will not be used if self managed dns is off.
	#[must_use]
	pub const fn raw_primary_dns(&self) -> Ipv4Addr {
		self.primary_dns_server
	}
	/// Get the secondary dns server the cat-dev will be using.
	///
	/// *note: this will return none if the cat-dev isn't using self managed
	/// dns.*
	#[must_use]
	pub const fn secondary_dns(&self) -> Option<Ipv4Addr> {
		if self.dns {
			Some(self.secondary_dns_server)
		} else {
			None
		}
	}
	/// Get the raw value of the Secondary DNS Server for the cat-dev.
	///
	/// *note: this value will not be used if self managed dns is off.
	#[must_use]
	pub const fn raw_secondary_dns(&self) -> Ipv4Addr {
		self.secondary_dns_server
	}

	/// If we're using jumbo frames.
	#[must_use]
	pub const fn jumbo_frame(&self) -> bool {
		self.jumbo_frame
	}

	/// The potential IP Address of the host machine we're using.
	#[must_use]
	pub const fn host_pc_ip_address(&self) -> Ipv4Addr {
		self.host_pc_ip
	}

	/// Get the hard disk size of the cat-dev.
	#[must_use]
	pub const fn hdd_bank_size(&self) -> CatDevBankSize {
		self.bank_size
	}

	/// Get the bank number being used for the HDD.
	#[must_use]
	pub const fn hdd_bank_no(&self) -> u8 {
		self.hdd_bank_no
	}

	/// The port for the ATAPI emulator.
	#[must_use]
	pub const fn atapi_emulator_port(&self) -> u16 {
		self.atapi_emulator_port
	}

	/// The port for SDIO Printf/Control.
	#[must_use]
	pub const fn sdio_printf_port(&self) -> u16 {
		self.sdio_printf_port
	}

	/// The port for SDIO Block Data.
	#[must_use]
	pub const fn sdio_block_port(&self) -> u16 {
		self.sdio_block_port
	}

	/// Get the port for EXI traffic.
	#[must_use]
	pub const fn exi_port(&self) -> u16 {
		self.exi_port
	}

	/// Get the port to use for parameter-space queries.
	#[must_use]
	pub const fn parameter_space_port(&self) -> u16 {
		self.parameter_space_port
	}

	/// If we're using drive-timing emulation of a production console.
	#[must_use]
	pub const fn drive_timing_emulation_enabled(&self) -> bool {
		self.drive_timing_emulation
	}

	/// Check if our operational mode is CAT-DEV mode.
	#[must_use]
	pub const fn is_cat_dev_mode(&self) -> bool {
		!self.operational_mode_is_reader
	}
	/// Check if our operational mode is in H-Reader mode.
	#[must_use]
	pub const fn is_h_reader_mode(&self) -> bool {
		self.operational_mode_is_reader
	}

	/// Get the hard-drive product revision.
	#[must_use]
	pub const fn drive_product_revision(&self) -> &String {
		&self.drive_product_revision
	}

	/// Get the hard-drive vendor code.
	#[must_use]
	pub const fn drive_vendor_code(&self) -> &String {
		&self.drive_vendor_code
	}

	/// Get the hard-drive device code.
	#[must_use]
	pub const fn drive_device_code(&self) -> &String {
		&self.drive_device_code
	}

	/// Get the release date of the internal hard-drive.
	#[must_use]
	pub const fn drive_release_date(&self) -> &String {
		&self.drive_release_date
	}

	/// Get the name of the cat-dev.
	#[must_use]
	pub const fn device_name(&self) -> &String {
		&self.device_name
	}

	#[must_use]
	pub const fn mac_address(&self) -> MacAddress {
		self.mac_address
	}
}

/// The bank sizes a cat-dev hard disk can have normally.
///
/// The blank value is a bit of a weird value, that i'm not sure why they
/// ever had it, but it shows up in the `setup.cgi` page, so whatever they
/// must have had a reason for it.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum CatDevBankSize {
	/// Unknown choice present in `setup.cgi`
	Blank,
	/// The internal hard-disk has 25 Gigabytes of storage.
	TwentyFiveGbs,
	/// The internal hard-disk has 5 Gigabytes of storage.
	FiveGbs,
	/// The internal hard-disk has 9 Gigabytes of storage.
	NineGbs,
	/// The internal hard-disk has 12 Gigabytes of storage.
	TwelveGbs,
	/// The internal hard-disk has 14 Gigabytes of storage.
	FourteenGbs,
	/// The internal hard-disk has 16 Gigabytes of storage.
	SixteenGbs,
	/// The internal hard-disk has 18 Gigabytes of storage.
	EighteenGbs,
	/// The internal hard-disk has 21 Gigabytes of storage.
	TwentyOneGbs,
}
impl From<&CatDevBankSize> for u32 {
	fn from(size: &CatDevBankSize) -> u32 {
		match *size {
			CatDevBankSize::Blank => 4_294_967_295,
			CatDevBankSize::TwentyFiveGbs => 0,
			CatDevBankSize::FiveGbs => 1,
			CatDevBankSize::NineGbs => 2,
			CatDevBankSize::TwelveGbs => 3,
			CatDevBankSize::FourteenGbs => 4,
			CatDevBankSize::SixteenGbs => 5,
			CatDevBankSize::EighteenGbs => 6,
			CatDevBankSize::TwentyOneGbs => 7,
		}
	}
}
impl From<CatDevBankSize> for u32 {
	fn from(value: CatDevBankSize) -> Self {
		Self::from(&value)
	}
}
impl TryFrom<u32> for CatDevBankSize {
	type Error = MIONCGIApiError;

	fn try_from(value: u32) -> Result<Self, Self::Error> {
		match value {
			u32::MAX => Ok(Self::Blank),
			0 => Ok(Self::TwentyFiveGbs),
			1 => Ok(Self::FiveGbs),
			2 => Ok(Self::NineGbs),
			3 => Ok(Self::TwelveGbs),
			4 => Ok(Self::FourteenGbs),
			5 => Ok(Self::SixteenGbs),
			6 => Ok(Self::EighteenGbs),
			7 => Ok(Self::TwentyOneGbs),
			_ => Err(MIONCGIApiError::UnknownCatDevBankSizeId(value)),
		}
	}
}
impl Display for CatDevBankSize {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		write!(
			fmt,
			"{}",
			match *self {
				Self::Blank => " ",
				Self::TwentyFiveGbs => "25GB",
				Self::FiveGbs => "5GB",
				Self::NineGbs => "9GB",
				Self::TwelveGbs => "12GB",
				Self::FourteenGbs => "14GB",
				Self::SixteenGbs => "16GB",
				Self::EighteenGbs => "18GB",
				Self::TwentyOneGbs => "21GB",
			}
		)
	}
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[test]
	pub fn convert_bank_size() {
		for bank_size in vec![
			CatDevBankSize::Blank,
			CatDevBankSize::TwentyFiveGbs,
			CatDevBankSize::FiveGbs,
			CatDevBankSize::NineGbs,
			CatDevBankSize::TwelveGbs,
			CatDevBankSize::FourteenGbs,
			CatDevBankSize::SixteenGbs,
			CatDevBankSize::EighteenGbs,
			CatDevBankSize::TwentyOneGbs,
		] {
			assert_eq!(
				Ok(bank_size),
				CatDevBankSize::try_from(u32::from(bank_size)),
				"bank size: {} was not the same after converting it back & forth",
				bank_size,
			);
		}
	}
}
