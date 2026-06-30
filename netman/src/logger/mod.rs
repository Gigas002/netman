// SPDX-License-Identifier: GPL-3.0-only

//! Tracing subscriber initialisation from resolved `Settings`.
//!
//! Called exactly once from `main`, immediately after settings resolution.
//! No other module configures or re-initialises the subscriber.

use std::fs::OpenOptions;

use anyhow::{Context, Result};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::settings::Settings;

/// Initialise the global tracing subscriber.
///
/// When `settings.log_file` is set, log output goes to that file;
/// otherwise it goes to stderr. The format layer uses compact output to
/// keep TUI rendering unobstructed (the TUI draws over stderr in raw mode,
/// so file logging is strongly recommended in production).
pub fn init(settings: &Settings) -> Result<()> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&settings.log_level));

    match &settings.log_file {
        Some(path) => {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .with_context(|| format!("opening log file {}", path.display()))?;

            // Ignore "subscriber already set" — can happen in test runners.
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().with_writer(file).with_ansi(false).compact())
                .try_init();
        }
        None => {
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().with_writer(std::io::stderr).compact())
                .try_init();
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests;
