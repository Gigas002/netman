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

## [0.1.0] - 2026-09-04
