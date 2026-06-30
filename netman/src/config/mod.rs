// SPDX-License-Identifier: GPL-3.0-only

//! TOML configuration file loading.
//!
//! This module only reads and deserializes a file.  It applies no precedence
//! rules and performs no merging — that is `settings`' responsibility.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Raw configuration values from a TOML file.
///
/// All fields are `Option` so that any subset of the file may be omitted;
/// `settings::resolve` supplies defaults for anything left unset.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    #[serde(default)]
    pub log: LogConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub network: NetworkConfig,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogConfig {
    /// Minimum log level: "error", "warn", "info", "debug", "trace".
    pub level: Option<String>,
    /// Path for log output; stderr when absent.
    pub file: Option<PathBuf>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiConfig {
    /// UI tick rate in milliseconds.
    pub tick_rate: Option<u64>,
    /// Show the detail panel on startup.
    pub show_detail: Option<bool>,
    /// Number of rows in the connection list before scrolling.
    pub list_height: Option<u16>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkConfig {
    /// Enable periodic Wi-Fi scan requests.
    pub auto_scan: Option<bool>,
    /// Interval between automatic scans in seconds.
    pub scan_interval: Option<u64>,
}

/// Load the configuration file at `path` (or the XDG default location when
/// `path` is `None`).  Returns `None` when no file is found.
pub fn load(path: Option<&Path>) -> Result<Option<FileConfig>> {
    let resolved = match path {
        Some(p) => Some(p.to_owned()),
        None => xdg_default_path(),
    };

    let Some(file) = resolved else {
        return Ok(None);
    };

    if !file.exists() {
        debug!(path = %file.display(), "config file not found — using defaults");
        return Ok(None);
    }

    debug!(path = %file.display(), "loading config file");
    let raw = std::fs::read_to_string(&file)
        .with_context(|| format!("reading config file {}", file.display()))?;
    let cfg: FileConfig =
        toml::from_str(&raw).with_context(|| format!("parsing config file {}", file.display()))?;
    Ok(Some(cfg))
}

fn xdg_default_path() -> Option<PathBuf> {
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".config"))
        })?;
    Some(base.join("netman").join("config.toml"))
}

#[cfg(test)]
mod tests;
