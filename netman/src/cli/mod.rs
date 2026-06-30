// SPDX-License-Identifier: GPL-3.0-only

//! Command-line argument definitions.
//!
//! Parsing happens exclusively here and in `main`; the result is handed to
//! `settings::resolve` and never referenced after that point.

use clap::Parser;

/// TUI applet analogue of network-manager-applet.
#[derive(Debug, Parser)]
#[command(name = "netman", author, version, about, long_about = None)]
pub struct CliOptions {
    /// Path to the TOML configuration file.
    ///
    /// Defaults to `$XDG_CONFIG_HOME/netman/config.toml` or
    /// `~/.config/netman/config.toml` when not supplied.
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<std::path::PathBuf>,

    /// Increase log verbosity (repeat for more: -v = debug, -vv = trace).
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Write log output to this file instead of stderr.
    #[arg(long, value_name = "FILE")]
    pub log_file: Option<std::path::PathBuf>,

    /// Tick rate in milliseconds — how often the UI polls for NM state changes.
    #[arg(long, default_value_t = 1000, value_name = "MS")]
    pub tick_rate: u64,
}

/// Parse `argv` using clap and return the structured options.
pub fn parse() -> CliOptions {
    CliOptions::parse()
}

#[cfg(test)]
mod tests;
