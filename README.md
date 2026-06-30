# netman

> TUI applet analogue of `network-manager-applet`.

`netman` provides a keyboard-driven interface for managing Wi-Fi, Ethernet, and
VPN connections through NetworkManager, using the same D-Bus API as the
GNOME panel applet.

## Features

- List all saved and in-range network connections grouped by type (Wi-Fi, Ethernet, VPN)
- Real-time signal strength bars and connection status
- Connect and disconnect with a single keypress
- Detail panel: IP address, gateway, DNS, BSSID, band, security type

## Workspace layout

```
netman/       — binary crate (TUI binary)
libnetman/    — library crate (domain types + D-Bus integration)
examples/     — example TOML configs
docs/         — architecture and user documentation
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
| `r` / `F5` | Refresh connection list |
| `Tab` / `p` | Toggle detail panel |
| `?` | Toggle help overlay |
| `q` | Quit |
| `Ctrl+C` | Force quit |

## Configuration

Place a TOML file at `~/.config/netman/config.toml` (or pass `-c FILE`).
See [`examples/netman.toml`](examples/netman.toml) for all options.

## License

`netman` is free software licensed under the [GNU General Public License v3.0 only](LICENSE).
