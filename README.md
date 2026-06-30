# netman

> TUI applet analogue of `network-manager-applet`.

`netman` provides a keyboard-driven interface for managing Wi-Fi, Ethernet, and
VPN connections through NetworkManager, using the same D-Bus API as the
GNOME panel applet.

## Requirements

- A running **NetworkManager** daemon (for live operation)
- Build with the default `dbus` feature enabled (see [Building](#building))

Without NetworkManager or without the `dbus` feature, `netman` starts in **demo
mode** with sample data — useful for UI development, but connect/disconnect,
scan, and profile changes are disabled.

## Features

### Connection list

- Saved profiles and in-range Wi-Fi access points, grouped by type (Wi-Fi,
  Ethernet, VPN)
- Real-time signal strength bars and connection status
- Visible-but-unsaved networks (connect-only; no edit or delete)
- `[ Connect to hidden… ]` entry for hidden Wi-Fi networks

### Connect and disconnect

- Connect / disconnect with a single keypress
- Password prompt for secured networks without saved credentials
- Transient status while activating or deactivating

### Networking controls

- Toggle all networking (`n`) and the Wi-Fi radio (`w`)
- Wi-Fi scan (`r` / `F5`) merges live AP data into the list

### Profiles

- **Edit** (`e`) saved Wi-Fi, Ethernet, and VPN profiles
- **Add** (`a`) new Wi-Fi, Ethernet, or VPN connections
- **Delete** (`D`) saved profiles with confirmation
- VPN: pick an installed plugin, import a file (`.ovpn`, etc.), or enter basic
  manual settings (gateway, username, password)

### Detail panel

- IPv4 and IPv6 address, gateway, and DNS (when connected)
- Wi-Fi: BSSID, band, security type
- VPN: plugin service type

## Out of scope (v1)

`netman` targets everyday Wi-Fi / Ethernet / VPN use — not the full
NetworkManager or `nm-connection-editor` surface. Not supported in v1:

- Mobile broadband / modems (planned as optional `mobile` feature)
- 802.1X enterprise Wi-Fi and WEP
- Full per-plugin VPN property matrices
- IPv6 profile editing (display only; new profiles set IPv6 to ignore)
- Bridges, bonds, VLANs, PPPoE, Bluetooth PAN, hotspots
- Captive portal browser launch
- Spawning external editors (`nm-connection-editor`)

See [`docs/PLAN.md`](docs/PLAN.md) for the implementation roadmap.

## Workspace layout

```
netman/       — binary crate (TUI binary)
libnetman/    — library crate (domain types + D-Bus integration)
examples/     — example TOML configs
docs/         — architecture and implementation plan
```

## Building

```sh
# Default build (D-Bus enabled — required for live NM interaction)
cargo build --release

# Minimal build (demo mode only; no D-Bus dependency)
cargo build --release --no-default-features
```

## Usage

```
netman [OPTIONS]

Options:
  -c, --config <FILE>   Config file [default: ~/.config/netman/config.toml]
  -v, --verbose         Increase log level (-v = debug, -vv = trace)
      --log-file <FILE> Write logs to file (recommended with TUI)
      --tick-rate <MS>  UI refresh interval [default: 1000]
  -h, --help            Print help
  -V, --version         Print version
```

### Keybindings

| Key | Action |
|-----|--------|
| `↑` / `k` | Move selection up |
| `↓` / `j` | Move selection down |
| `Enter` | Connect to selected network |
| `d` / `Del` | Disconnect selected network |
| `D` | Delete selected saved profile (with confirmation) |
| `e` | Edit selected saved profile |
| `a` | Add new connection |
| `r` / `F5` | Scan for Wi-Fi networks |
| `n` | Toggle networking on/off |
| `w` | Toggle Wi-Fi radio on/off |
| `Tab` / `p` | Toggle detail panel |
| `?` | Toggle help overlay |
| `Esc` | Close overlay |
| `q` | Quit |
| `Ctrl+C` | Force quit |

In modals (password prompt, connection editor, etc.): `Esc` cancels, `Enter`
confirms, `Tab` / `Shift+Tab` moves between fields. Password fields support
`Ctrl+H` to show/hide and `Ctrl+V` to paste.

## Configuration

Place a TOML file at `~/.config/netman/config.toml` (or pass `-c FILE`).
See [`examples/netman.toml`](examples/netman.toml) for all options.

## License

`netman` is free software licensed under the [GNU General Public License v3.0 only](LICENSE).
