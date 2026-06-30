// Integration tests for NmClient require a live NetworkManager daemon.
// They are marked #[ignore] and must be run manually on a Linux system:
//
//   cargo test -p libnetman --features dbus -- --ignored

use crate::connection::WifiSecurity;

use super::security_from_ap;

#[test]
#[ignore = "requires live NetworkManager daemon"]
fn placeholder_nm_connect() {
    // This test is intentionally empty; the #[ignore] attribute ensures it
    // does not run in CI. When NM is available run with --ignored to exercise
    // the full D-Bus path.
}

#[test]
fn security_from_ap_open() {
    assert_eq!(security_from_ap(0, 0, 0), WifiSecurity::None);
}

#[test]
fn security_from_ap_wpa2_psk() {
    assert_eq!(security_from_ap(0, 0, 0x102), WifiSecurity::Wpa2);
}

#[test]
fn security_from_ap_wpa3_sae() {
    assert_eq!(security_from_ap(0, 0, 0x400), WifiSecurity::Wpa3);
}

#[test]
fn security_from_ap_enterprise() {
    assert_eq!(security_from_ap(0, 0, 0x200), WifiSecurity::Enterprise);
}
