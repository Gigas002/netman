// SPDX-License-Identifier: GPL-3.0-only
use thiserror::Error;

/// Unified error type for `libnetman`.
#[derive(Debug, Error)]
pub enum Error {
    #[error("D-Bus error: {0}")]
    DBus(String),

    #[error("NetworkManager is not running or not reachable")]
    NmUnavailable,

    #[error("device not found: {0}")]
    DeviceNotFound(String),

    #[error("connection not found: {0}")]
    ConnectionNotFound(String),

    #[error("access point not found: {0}")]
    AccessPointNotFound(String),

    #[error("invalid state: {0}")]
    InvalidState(String),

    #[error("operation failed: {0}")]
    OperationFailed(String),
}

#[cfg(feature = "dbus")]
impl From<zbus::Error> for Error {
    fn from(e: zbus::Error) -> Self {
        Self::DBus(e.to_string())
    }
}

#[cfg(feature = "dbus")]
impl From<zbus::fdo::Error> for Error {
    fn from(e: zbus::fdo::Error) -> Self {
        Self::DBus(e.to_string())
    }
}

#[cfg(test)]
mod tests;
