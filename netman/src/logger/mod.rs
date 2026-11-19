// SPDX-License-Identifier: GPL-3.0-only

//! Tracing subscriber initialisation from resolved `Settings`.
//!
//! Called exactly once from `main`, immediately after settings resolution.
//! No other module configures or re-initialises the subscriber.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::settings::Settings;

/// Set while the TUI owns the terminal (see [`set_terminal_in_use`]).
static TERMINAL_IN_USE: AtomicBool = AtomicBool::new(false);

/// Tell the logger whether the TUI is drawing on the terminal.
///
/// Log lines written to stderr meanwhile would land in the middle of the
/// screen, so they are dropped until the terminal is restored. Logging to a
/// file (`--log-file`) is not affected.
pub fn set_terminal_in_use(in_use: bool) {
    TERMINAL_IN_USE.store(in_use, Ordering::Relaxed);
}

/// A writer that drops everything while `gate` is set.
struct Gated<W> {
    gate: &'static AtomicBool,
    inner: W,
}

impl<W: Write> Write for Gated<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.gate.load(Ordering::Relaxed) {
            return Ok(buf.len());
        }
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn stderr_unless_tui() -> Gated<io::Stderr> {
    Gated {
        gate: &TERMINAL_IN_USE,
        inner: io::stderr(),
    }
}

/// Initialise the global tracing subscriber.
///
/// When `settings.log_file` is set, log output goes to that file;
/// otherwise it goes to stderr, except while the TUI is on screen. The format layer uses compact output to
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
                .with(fmt::layer().with_writer(stderr_unless_tui).compact())
                .try_init();
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests;
