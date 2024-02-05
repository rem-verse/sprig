#![doc = include_str!("../README.md")]

use miette::{miette, Context, IntoDiagnostic, Result};
use once_cell::sync::Lazy;
use std::{env::var as env_var, net::SocketAddr, sync::Mutex};
use tracing::debug;
use tracing_error::ErrorLayer;
use tracing_subscriber::{
	fmt::layer as tracing_fmt_layer, prelude::*, registry as subscriber_registry, EnvFilter,
};

/// Check if we have actually initialized logging before.
static HAS_INITIALIZED_LOGGING: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));

/// Determine if our logger will use ANSI escape codes.
///
/// Although i wish we had support for better detection than this, this is
/// unfortunately inhereted from tracing-subscriber which is significantly
/// better at like everything else.
///
/// So even though it kinda stinks here, it's worth the tradeoff. This line is
/// copied directly from tracing subscriber:
/// <https://github.com/tokio-rs/tracing/blob/07b490067c0e2af61f48a3d2afb85a20ab70ba95/tracing-subscriber/src/fmt/fmt_subscriber.rs#L697>
#[must_use]
pub fn will_ansi() -> bool {
	env_var("NO_COLOR").map_or(true, |v| v.is_empty())
}

/// Install all the logging configuration needed for an application.
///
/// This should only ever be called as the very first part of `main`, and
/// nowhere else. If you try to call it elsewhere, you'll just get am error.
///
/// See the tracing docs for logging for more information:
/// <https://docs.rs/tracing/latest/tracing/#shorthand-macros>
///
/// It should be noted this is also what starts up 'tokio-console' level
/// filtering as that hooks very heavily into tracing data.
///
/// # Panics
///
/// If you've requested `tokio-console`, and it can't spawn the server.
///
/// # Errors
///
/// If we fail to install all of the logging handlers.
pub fn install_logging_handlers(use_json: bool) -> Result<()> {
	{
		let mut locked_init = HAS_INITIALIZED_LOGGING
			.lock()
			.expect("Intall logging handlers called with poisioned mutex?");
		if *locked_init {
			return Err(miette!("Logging has already been initialized!"));
		}
		*locked_init = true;
	}
	let explicit_level = env_var("SPRIG_LOGGING").ok();
	let console_address = env_var("SPRIG_TOKIO_CONSOLE_ADDR").ok();

	// tokio-console requires tokio/runtime to be at the trace level.
	let filter_layer = EnvFilter::try_from_default_env().or_else(|_| {
		EnvFilter::try_new(if let Some(el) = explicit_level {
			el
		} else if console_address.is_some() {
			"info,tokio=trace,runtime=trace".to_owned()
		} else {
			"info".to_owned()
		})
		.into_diagnostic()
	})?;
	let registry = subscriber_registry().with(filter_layer);

	if let Some(addr) = console_address.as_ref() {
		let console_uri = addr
			.parse::<SocketAddr>()
			.into_diagnostic()
			.wrap_err("Failed to parse `SPRIG_TOKIO_CONSOLE_ADDR` as an address to listen on!")?;

		if use_json {
			registry
				.with(tracing_fmt_layer().with_target(false).json())
				.with(ErrorLayer::default())
				.with(
					console_subscriber::ConsoleLayer::builder()
						.enable_self_trace(true)
						.server_addr(console_uri)
						.spawn(),
				)
				.init();
		} else {
			registry
				.with(tracing_fmt_layer().with_target(true))
				.with(ErrorLayer::default())
				.with(
					console_subscriber::ConsoleLayer::builder()
						.enable_self_trace(true)
						.server_addr(console_uri)
						.spawn(),
				)
				.init();
		}
	} else if use_json {
		registry
			.with(tracing_fmt_layer().with_target(true).json())
			.with(ErrorLayer::default())
			.init();
	} else {
		registry
			.with(tracing_fmt_layer().with_target(true))
			.with(ErrorLayer::default())
			.init();
	}

	debug!(
		console_enabled = console_address.is_some(),
		"tokio-console-status"
	);
	Ok(())
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[test]
	pub fn cant_install_twice() {
		assert!(
			install_logging_handlers(true).is_ok(),
			"Failed to perform initial install of logging handlers, this should ALWAYS succeed.",
		);
		assert!(
			install_logging_handlers(false).is_err(),
			"Second call to install of logging handlers somehow failed?",
		);
		assert!(
			install_logging_handlers(true).is_err(),
			"Third call to install of logging handlers somehow failed?",
		);
	}
}
