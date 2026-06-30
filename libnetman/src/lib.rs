// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2024 netman contributors
//
// netman is free software: you can redistribute it and/or modify it under the
// terms of the GNU General Public License as published by the Free Software
// Foundation, version 3.

//! Core library for `netman` — TUI applet analogue of network-manager-applet.
//!
//! This crate provides:
//! - [`connection`]: domain types for network connections, devices, and access
//!   points.
//! - [`error`]: unified error type.
//! - [`nm`] *(feature = `dbus`)*: live NetworkManager interaction via D-Bus.
//! - Mobile broadband types and NM modem integration *(feature = `mobile`)*.
//!
//! Without the `dbus` feature the library compiles to pure types and helpers
//! that can be used with a mock back-end or in tests.

pub mod connection;
pub mod error;
pub mod vpn_plugins;

#[cfg(feature = "dbus")]
pub mod nm;

pub use error::Error;
pub type Result<T> = std::result::Result<T, Error>;
