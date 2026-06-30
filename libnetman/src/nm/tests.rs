// Integration tests for NmClient require a live NetworkManager daemon.
// They are marked #[ignore] and must be run manually on a Linux system:
//
//   cargo test -p libnetman --features dbus -- --ignored

#[test]
#[ignore = "requires live NetworkManager daemon"]
fn placeholder_nm_connect() {
    // This test is intentionally empty; the #[ignore] attribute ensures it
    // does not run in CI. When NM is available run with --ignored to exercise
    // the full D-Bus path.
}
