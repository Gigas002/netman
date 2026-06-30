# Changelog

All notable changes to `netman` are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added

- Initial project from `rust-template`; renamed crates to `netman` (binary)
  and `libnetman` (library).
- **`libnetman`**: domain types for `Connection`, `WifiInfo`, `VpnInfo`,
  `NmState`, `ConnectivityState`, `Ip4Config`, `WifiSecurity`, `WifiMode`,
  and `ConnectionStatus`.
- **`libnetman`** *(feature `dbus`)*: `NmClient` — async NetworkManager client
  via `zbus`; D-Bus proxy definitions for `NetworkManager`, `Settings`,
  `SettingsConnection`, `ActiveConnection`, `Device`, `DeviceWireless`,
  `AccessPoint`, and `IP4Config` interfaces.
- **`netman`**: full TUI binary with:
  - `cli/` — `clap`-based argument parsing.
  - `config/` — TOML config loading with XDG path resolution.
  - `settings/` — three-layer resolver (CLI > config > defaults).
  - `logger/` — `tracing-subscriber` initialisation (stderr or file).
  - `app/` — async event loop (`tokio`), demo mode fallback, keyboard handling.
  - `ui/` — `ratatui`/`crossterm` rendering: connection list, detail panel,
    status bar, key hints, help overlay.
- GPL-3.0-only license.
- Example configuration at `examples/netman.toml`.
