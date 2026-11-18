# Changelog

All notable changes to `netman` are documented here.

---

## [Unreleased]

### Fixed

- Connecting, disconnecting, connecting to a new network and saving an edited
  profile failed against NetworkManager: object paths were sent as strings, and
  `Update2` was called without its `args` parameter, so NM rejected the calls
  with `InvalidArgs`.

## [0.1.0] - 2026-09-04
