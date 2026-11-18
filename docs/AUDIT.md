# Audit

A whole-application audit of netman and libnetman (2026-11-19) in five parts:
bugs (§2), performance (§3), security and robustness (§4), usability (§5),
and code quality, tests and project hygiene (§6). §7 lists what was left open,
and §8 is the checklist to re-run the audit.

Each finding says what was found, how serious it is, and what was done:
**fixed** (with the commit), **accepted** (with the reason), or **OK**
(checked, nothing to do). Every fix is its own commit, with a regression test
where one can be written.

Severity: **high** (a core action that never works against a real
NetworkManager, or a crash in normal use), **medium** (wrong behavior in a
reachable case, an ignored setting), **low** (hardening, polish, or needs an
unusual setup).

## 1. Method

Every source file was read in full. Nothing in the audit ran against the
machine's NetworkManager or system bus: every test and binary ran inside a
`bwrap` sandbox that hides `/run/dbus` and points `DBUS_SYSTEM_BUS_ADDRESS` at
a private `dbus-daemon`. Findings were reproduced in four ways:

- **A strict NM stub.** A GDBus service generated from NetworkManager's
  installed introspection XML (`/usr/share/dbus-1/interfaces`) type-checks
  every call like the real daemon. This is what showed that connect,
  disconnect, add-and-connect and save all sent the wrong argument types.
- **A populated mock NM.** `python-dbusmock`'s NetworkManager template, filled
  with 30 access points (WPA2/WPA3 transition, OWE, WPA1, 6 GHz, open, 802.1X,
  a Latin-1 SSID), 15 saved Wi-Fi profiles, an ethernet profile, two VPNs and
  one active connection. The release binary ran against it in a pty that
  measured CPU, context switches, RSS and output, and counted D-Bus method
  calls with `dbus-monitor`.
- **An in-process fake for tests.** `libnetman/src/nm/tests.rs` serves NM (and
  ModemManager for the `mobile` feature) over a peer-to-peer socket, with NM's
  real argument types and flag values, and records every call it receives.
  Library tests use it through `NmClient::with_connection`; they need no bus.
- **The release binary in a pty**, with the tty's `icanon`/`echo` checked after
  exit, for crashes, panics (injected, not committed), bracketed paste and log
  output.

Builds checked: `--no-default-features`, default, `--all-features`. Static
checks: `cargo clippy` at the three levels, `cargo deny check`, `typos`,
`cargo doc`, `cargo llvm-cov`.

## 2. Bugs

| Area | Finding | Status |
| ---- | ------- | ------ |
| D-Bus argument types | `ActivateConnection`, `DeactivateConnection` and `AddAndActivateConnection` took object paths as `&str`, so zbus sent `(sss)` where NM expects `(ooo)`. `Update2` lacked its `args` parameter and had the wrong return type. NM rejects mismatched calls with `InvalidArgs`, so **connecting, disconnecting, joining a new network and saving an edited profile all failed**. Reproduced against the strict stub. The test fake checks the same signatures. | Fixed, high (0b55a1e) |
| Networking toggle | `n` wrote the `NetworkingEnabled` property, which is read-only. NM switches networking with `Enable(b)`. The toggle never worked. | Fixed, high (4198761) |
| Access-point security | The flag bits were wrong. `0x1`/`0x2` are `PAIR_WEP40`/`PAIR_WEP104`, not "WPA"/"RSN", and SAE was checked before PSK. As a result, Enhanced Open (OWE) networks showed as WEP, which netman refuses to join. WPA2/WPA3 transition networks were joined as SAE, which fails on adapters without SAE. WPA1 showed as WPA2. OWE is now its own security type, joined without a password. | Fixed, high (0ff01e0) |
| Modem details | ModemManager proxies were built at NM's `Device.Modem.DeviceId`, which is a hash, not an object path. The ModemManager object is the device's `Udi`. Signal, technology, operator and SIM lock were never read, the PIN prompt never opened by itself, and sending a PIN always failed. | Fixed, high (0289da4) |
| Selection on refresh | The selection was a list position. Every refresh re-sorts Wi-Fi by signal strength, so the highlighted row changed under the cursor, and Enter, `d`, `D` or `e` could act on a different network. The selection now follows the connection's UUID. | Fixed, high (9df1ef7) |
| Scan freezes the UI | `request_wifi_scan` slept for 2 s inside the event loop: 2025 ms from `r` to the next frame. The request now returns at once, and "Scanning…" clears when the device's `LastScan` changes. The test uses tokio's paused clock. | Fixed, medium (d2d2ab2) |
| IPv6 DNS | It was read from `IP6Config.NameserverData`, which only IP4Config has; the error was swallowed. The servers are read from `Nameservers` (16-byte addresses) now. | Fixed, medium (3f95466) |
| Bands | Everything from 3 GHz up was labelled "5 GHz", including 6 GHz (Wi-Fi 6E). An unreadable frequency or BSSID showed as "2.4 GHz", "0 MHz" or an empty BSSID. | Fixed, low (4b8abdc) |
| Ignored scan settings | `[network] auto_scan` and `scan_interval` (on, every 30 s, per `examples/netman.toml`) were resolved and then never used. A periodic scan runs now while Wi-Fi is on. Against the mock with `scan_interval = 2`: 3 `RequestScan` calls in 7.5 s, none before. | Fixed, medium (8b6fc58) |
| Tick rate | `--tick-rate 1000` lost to the config file, because 1000 meant "not given". `--tick-rate 0` panicked tokio's interval after raw mode was on, which left the terminal raw. The flag is optional now, and 0 is rejected by clap and by config loading. | Fixed, medium (5da48a4) |
| Terminal left raw | Raw mode and the alternate screen were undone only on the normal exit path. A draw error or a panic left the shell raw, with the panic message staircased across it. A drop guard and a panic hook restore the terminal; checked in a pty with an injected panic. | Fixed, medium (3bb1c3f) |
| Logs over the TUI | Without `--log-file`, warnings went to stderr, which is the terminal being drawn on. When NM went away, "refresh failed" landed on the screen every second. Stderr output is now dropped while the TUI is up. | Fixed, medium (90c269c) |
| Paste | `Event::Paste` was handled, but bracketed paste was never enabled. Pasted text arrived as key presses, and a trailing newline submitted the form. It is enabled now (pty: `ESC[?2004h`/`l`), line breaks are dropped from pastes, and the README no longer promises `Ctrl+V`. | Fixed, medium (0351383) |
| Help overlay | A fixed 20 rows held 27 lines, so the General section (`q`, `Ctrl+C`) was never visible. The test fails on the old code. | Fixed, low (aefdf5d) |
| VPN plugins | With no NM VPN plugin installed, the add-VPN menu offered a sample OpenVPN entry meant for demo mode, and the profile it made couldn't work. It now says "No VPN plugins installed.". | Fixed, low (0114e7f) |
| Demo fallback | When NM can't be reached, netman shows sample data and says so in the status line. | OK |

## 3. Performance

Measured with release builds against the populated mock NM (§1), base =
master f2e4e67, final = this branch. Idle figures are 10 s windows, with three
runs per build. Context switches stand in for wake-ups.

| Metric | Base | Final |
| ------ | ---- | ----- |
| Idle CPU | 1.3–1.5 % | 0.2 % |
| Context switches (idle) | 637–643/s | 187–188/s |
| Terminal output (idle) | 0.49 KiB/s | 0.02 KiB/s |
| D-Bus method calls in 10 s idle | 2278 | 735 |
| D-Bus calls per refresh (`connections()`) | 161.7 | 55.2 (identical results, diffed) |
| `connections()` median, n = 100 | 11.1 ms | 7.0 ms |
| `r` to next frame | 2026 ms | 5 ms |
| Start-up to first frame | 20 ms | 10–15 ms |
| RSS | 7.5–7.6 MiB | 7.7–7.8 MiB |
| Release binary | 8.10 MB | 8.19 MB |

| Area | Finding | Status |
| ---- | ------- | ------ |
| Idle redraws | Each loop pass spawned a blocking task that polled the terminal for 50 ms, then redrew: about 20 frames a second with nothing changing. A thread now blocks on terminal input, and the loop draws once per wake-up. CPU 1.35 → 0.65 %, output 0.49 → 0.02 KiB/s. | Fixed, medium (8c6acd8) |
| D-Bus traffic | Every call built short-lived proxies with zbus' default property cache. The first read on such a proxy subscribes and fetches everything (GetNameOwner, AddMatch, GetAll), then RemoveMatch on drop, for about 38 proxies per refresh. Proxies are uncached now. Objects with several properties are read with one `GetAll`, and each active connection once per refresh. A test pins one call per object per refresh. | Fixed, medium (8046eb7) |
| Refresh tick | The 1 s `tick_rate` refresh is the remaining idle cost. Switching to NM's `PropertiesChanged` signals would remove it, but that is a redesign of the refresh model (§7). | Accepted |
| RSS | The input thread and channel add about 0.2 MiB over the base, net of the proxy change. | Accepted |

Not measured: anything against a real NetworkManager or a real radio (not
allowed by the audit's rules; the mock is Python, so absolute times are the
mock's, and the call counts are what transfers); the `mobile` path's cost
(the fake has one modem); and lists much larger than 30 access points.

## 4. Security and robustness

| Area | Finding | Status |
| ---- | ------- | ------ |
| Secrets | Passwords and PINs go to NM in the settings dictionary over the system bus, which is how NM takes them. No log call includes settings or secrets. They are not zeroed in memory after use. | Accepted, low |
| VPN import | `nmcli connection import` runs with an argument vector, not a shell. The plugin name comes from the installed-plugin list, and the file is checked to exist first. | OK |
| Bad config values | Unknown keys are already errors. `tick_rate = 0` is now one too (5da48a4). | OK |
| Panics | No `unwrap` on D-Bus data in the read path: unreadable objects are skipped with a warning. The terminal is restored on any panic (3bb1c3f). | OK |
| Dependencies | `cargo deny check` passes and the lockfile is current. | OK |

## 5. Usability

| Area | Finding | Status |
| ---- | ------- | ------ |
| Logging advice | `examples/netman.toml` says logs are dropped while the TUI is up, and to set `file` to keep them. | Fixed, docs (90c269c) |
| `--help` | `--tick-rate` says where its default comes from. | Fixed (5da48a4) |
| `list_height` | Documented as unused; it is read and ignored. | Accepted |
| Hidden networks | The hidden-network prompt always creates a WPA2-PSK profile. Open, WPA3 and OWE hidden networks need the editor afterwards. | Accepted, low, §7 |

## 6. Code quality, tests and hygiene

| Area | Finding | Status |
| ---- | ------- | ------ |
| NM client untested | `nm/mod.rs` had 2.9 % line coverage, and nothing checked argument types. The p2p fake now covers activation, deactivation, add-and-activate, update, delete, the networking and Wi-Fi toggles, scans, access-point security, IP details and the modem path. | Fixed (0b55a1e onwards) |
| Coverage | 20.06 % → 36.86 % of lines (`cargo llvm-cov --workspace --all-features`). `nm/mod.rs`: 2.9 → 72.4 %. `app/mod.rs` (18.6 %) is mostly async handlers that need a client, and `ui/` mostly drawing. | Fixed, partly; §7 |
| Duplicated logic | `detect_wifi_security` existed twice, with different results for OWE. | Fixed (0ff01e0) |
| Cargo warnings | Every build printed three manifest warnings: `homepage` repeating `repository`, an unused workspace `description`, and `tracing` unused without `dbus`. | Fixed (b3ff7ef) |
| Flaky tests | The new tests don't wait on wall-clock time: the scan test uses tokio's paused clock, and the call log waits on a marker call. The suites passed 30 parallel runs each. | OK |
| Gates | fmt, typos, deny, clippy and tests at the three feature levels, and docs, pass on every commit. | OK |

## 7. Left open

- **Signals instead of polling.** netman refreshes everything every
  `tick_rate` (1 s). Subscribing to NM's `PropertiesChanged` and
  `StateChanged` signals would leave it idle until something changes, and
  would make `tick_rate` a fallback. It is the largest remaining idle cost
  (§3), but a change in design rather than a fix.
- **Non-UTF-8 SSIDs.** An access point whose SSID isn't UTF-8 is skipped
  (`String::from_utf8` fails), so it can't be joined from netman. Showing it
  lossily needs a byte-SSID path through `WifiInfo` and the profile builders.
- **Hidden network security.** The prompt could offer a security choice
  instead of always using WPA2-PSK.
- **App coverage.** The async `App` handlers could run against the p2p fake,
  the way the library tests do, which would cover most of `app/mod.rs`.
- **Repository URL.** The manifests point at `github.com/netman-rs/netman`,
  while this checkout's origin is `github.com/Gigas002/netman`. Not changed:
  it may be the intended home.

## 8. Re-running the audit

```sh
# Gates
cargo fmt --all -- --check && typos && cargo deny check
for f in --no-default-features "" --all-features; do
  cargo clippy --workspace --all-targets $f -- -D warnings
  cargo test --workspace $f
done
cargo doc --workspace --no-deps

# Coverage (system LLVM tools on Arch: LLVM_COV / LLVM_PROFDATA)
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata \
  cargo llvm-cov --workspace --all-features --summary-only
```

The library tests need no bus. To run netman itself against a mock rather
than the machine's NetworkManager, start a private bus and
`python3 -m dbusmock --template networkmanager` on it. Then run netman under
`bwrap --dev-bind / / --tmpfs /run/dbus --setenv DBUS_SYSTEM_BUS_ADDRESS <the
private bus>`, so it can't reach the real one even if the variable is lost.
`dbus-monitor` on the private bus counts the calls behind §3.
