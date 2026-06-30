// SPDX-License-Identifier: GPL-3.0-only

//! Unified settings resolver.
//!
//! Merges CLI options and config-file values over built-in defaults into a
//! single `Settings` struct that the rest of the application consumes.
//! No module below this layer may accept `CliOptions`, raw `clap` types, or
//! unparsed config paths.

use std::path::PathBuf;

use crate::{cli::CliOptions, config::FileConfig};

/// Fully resolved application settings. The single source of truth passed
/// through the entire application after `main` sets up infrastructure.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Minimum tracing log level.
    pub log_level: String,
    /// Optional log output file (stderr when `None`).
    pub log_file: Option<PathBuf>,
    /// UI tick rate in milliseconds.
    pub tick_rate: u64,
    /// Show the connection detail panel on startup.
    pub show_detail: bool,
    /// Enable periodic Wi-Fi scan requests.
    pub auto_scan: bool,
    /// Interval between automatic scans in seconds.
    pub scan_interval: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            log_level: "warn".into(),
            log_file: None,
            tick_rate: 1000,
            show_detail: true,
            auto_scan: true,
            scan_interval: 30,
        }
    }
}

/// Merge CLI options and an optional config file into a `Settings` value.
///
/// Precedence: CLI > config file > built-in defaults.
pub fn resolve(cli: CliOptions, file: Option<FileConfig>) -> Settings {
    let defaults = Settings::default();
    let file = file.unwrap_or_default();

    let log_level = match cli.verbose {
        0 => file.log.level.unwrap_or_else(|| defaults.log_level.clone()),
        1 => "debug".into(),
        _ => "trace".into(),
    };

    let log_file = cli.log_file.or(file.log.file);

    let tick_rate = if cli.tick_rate != 1000 {
        cli.tick_rate
    } else {
        file.ui.tick_rate.unwrap_or(defaults.tick_rate)
    };

    let show_detail = file.ui.show_detail.unwrap_or(defaults.show_detail);
    let auto_scan = file.network.auto_scan.unwrap_or(defaults.auto_scan);
    let scan_interval = file.network.scan_interval.unwrap_or(defaults.scan_interval);

    Settings {
        log_level,
        log_file,
        tick_rate,
        show_detail,
        auto_scan,
        scan_interval,
    }
}

#[cfg(test)]
mod tests;
