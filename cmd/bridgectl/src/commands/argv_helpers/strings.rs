//! String utility helpers

use std::{fmt::Display, num::ParseIntError};

/// Get a string potentially padded with spaces, or cut off with '...'.
pub fn get_padded_string(ty: impl Display, max_length: usize) -> String {
	let mut to_return = String::with_capacity(max_length);
	let as_display = format!("{ty}");
	if as_display.len() > max_length {
		to_return.push_str(&as_display[..(max_length - 3)]);
		to_return.push_str("...");
	} else {
		to_return = as_display;
		while to_return.len() < max_length {
			to_return.push(' ');
		}
	}
	to_return
}

/// Get a byte value which could potentially be a hex value.
pub fn get_byte_value(value: &str) -> Result<u8, ParseIntError> {
	if let Some(hex_value) = value.strip_prefix("0x") {
		u8::from_str_radix(hex_value, 16)
	} else if value
		.to_lowercase()
		.chars()
		.any(|character| matches!(character, 'a'..='f'))
	{
		u8::from_str_radix(value, 16)
	} else {
		value.parse::<u8>()
	}
}
