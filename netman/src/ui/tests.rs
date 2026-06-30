// UI rendering tests: verify key helper functions produce expected output
// without requiring a live terminal (no frame rendering here).

use libnetman::connection::WifiInfo;
use libnetman::connection::{WifiMode, WifiSecurity};

#[test]
fn strength_bar_boundaries() {
    let make_wifi = |strength| WifiInfo {
        ssid: "test".into(),
        strength,
        security: WifiSecurity::None,
        frequency: None,
        bssid: None,
        mode: WifiMode::Infrastructure,
    };

    let full = make_wifi(100);
    let empty = make_wifi(0);
    assert_eq!(full.strength_bar(), "████");
    assert_eq!(empty.strength_bar(), "░░░░");

    for s in [25u8, 50, 75, 99] {
        let w = make_wifi(s);
        let bar = w.strength_bar();
        assert_eq!(bar.chars().count(), 4, "bar must be 4 chars wide at {s}%");
    }
}
