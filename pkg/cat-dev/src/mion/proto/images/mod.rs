//! Protocols, and types specifically related to "IMAGES", or Disc Images for
//! dealing with the MION.

use std::fmt::{Display, Formatter, Result as FmtResult};
use valuable::Valuable;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Valuable)]
pub enum MIONDiscImageType {
	WUMAD,
	WUM,
	Unknown(u8),
}
impl Display for MIONDiscImageType {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		match *self {
			Self::WUMAD => write!(fmt, "WUMAD"),
			Self::WUM => write!(fmt, "WUM/UNSET"),
			Self::Unknown(val) => write!(fmt, "Unk({val})"),
		}
	}
}
impl From<u8> for MIONDiscImageType {
	fn from(value: u8) -> Self {
		match value {
			255 => MIONDiscImageType::WUM,
			254 => MIONDiscImageType::WUMAD,
			num => MIONDiscImageType::Unknown(num),
		}
	}
}
impl From<MIONDiscImageType> for u8 {
	fn from(value: MIONDiscImageType) -> u8 {
		match value {
			MIONDiscImageType::WUM => 255,
			MIONDiscImageType::WUMAD => 254,
			MIONDiscImageType::Unknown(num) => num,
		}
	}
}
