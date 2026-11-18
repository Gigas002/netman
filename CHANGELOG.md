# Changelog

All notable changes to `netman` are documented here.

---

## [Unreleased]

### Fixed

- Connecting, disconnecting, connecting to a new network and saving an edited
  profile failed against NetworkManager: object paths were sent as strings, and
  `Update2` was called without its `args` parameter, so NM rejected the calls
  with `InvalidArgs`.
- Toggling networking (`n`) failed: it wrote the read-only `NetworkingEnabled`
  property instead of calling NM's `Enable` method.
- Wi-Fi security was read with the wrong NM flag bits. Enhanced Open (OWE)
  networks showed as WEP and could not be joined; WPA2/WPA3 transition networks
  were joined as WPA3-only, which fails on adapters without SAE; WPA1 networks
  showed as WPA2. OWE networks are now joined without a password.
- 6 GHz networks were labelled "5 GHz", and an access point with no reported
  frequency was labelled "2.4 GHz" with "0 MHz" in the detail panel.
- The detail panel never showed IPv6 DNS servers: they were read from
  `NameserverData`, which only IPv4 configs have.
- Mobile broadband (`mobile` feature): signal, technology, operator and SIM
  lock state were never shown, and SIM PIN unlock always failed. The
  ModemManager object was looked up by NM's `DeviceId`, an opaque hash, instead
  of the device's `Udi`.
- The selection kept its position while each refresh re-sorted Wi-Fi networks
  by signal, so Enter, `d`, `D` or `e` could act on a different network than
  the highlighted one a moment earlier. It now stays on the same connection.
- Scanning (`r` / `F5`) froze the UI for two seconds. The scan now runs in the
  background; new networks appear as NM finds them, and "Scanning…" clears
  when NM reports the scan finished (`LastScan`).
- `[network] auto_scan` and `scan_interval` were read but never used; netman
  now asks NM for a scan every `scan_interval` seconds (30 by default) while
  Wi-Fi is on.
- `--tick-rate 1000` did not override `[ui] tick_rate` from the config file
  (1000 was taken to mean "not given"). A tick rate of 0 crashed netman and
  left the terminal in raw mode; it is now rejected with an error.
- A panic, or an error while drawing, left the terminal in raw mode on the
  alternate screen. The terminal is now restored on every exit path, and
  before a panic message is printed.
- Without `--log-file`, warnings (for example "refresh failed" every second
  while NM restarts) were written to stderr over the TUI. They are now
  dropped while the TUI is on screen; use `--log-file` to keep them.
- Pasting never arrived as a paste: bracketed paste was not enabled, so pasted
  text came in as key presses and a trailing newline submitted the form. It is
  enabled now, and line breaks in pasted text are dropped. The README no longer
  promises `Ctrl+V`, which most terminals don't send as a paste.

## [0.1.0] - 2026-09-04
