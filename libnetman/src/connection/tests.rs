use super::*;

fn wifi_connection(strength: u8, security: WifiSecurity, status: ConnectionStatus) -> Connection {
    Connection {
        id: "Test WiFi".into(),
        uuid: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into(),
        kind: ConnectionKind::Wifi(WifiInfo {
            ssid: "TestNet".into(),
            strength,
            security,
            frequency: Some(5180),
            bssid: Some("aa:bb:cc:dd:ee:ff".into()),
            mode: WifiMode::Infrastructure,
        }),
        status,
        ip4: None,
        device: Some("wlan0".into()),
    }
}

#[test]
fn connection_label_returns_ssid_for_wifi() {
    let conn = wifi_connection(80, WifiSecurity::Wpa2, ConnectionStatus::Active);
    assert_eq!(conn.label(), "TestNet");
}

#[test]
fn connection_is_active() {
    let active = wifi_connection(80, WifiSecurity::Wpa2, ConnectionStatus::Active);
    let inactive = wifi_connection(80, WifiSecurity::Wpa2, ConnectionStatus::Inactive);
    assert!(active.is_active());
    assert!(!inactive.is_active());
}

#[test]
fn wifi_strength_bar_full() {
    let info = WifiInfo {
        ssid: "X".into(),
        strength: 100,
        security: WifiSecurity::None,
        frequency: None,
        bssid: None,
        mode: WifiMode::Infrastructure,
    };
    assert_eq!(info.strength_bar(), "████");
}

#[test]
fn wifi_strength_bar_empty() {
    let info = WifiInfo {
        ssid: "X".into(),
        strength: 0,
        security: WifiSecurity::None,
        frequency: None,
        bssid: None,
        mode: WifiMode::Infrastructure,
    };
    assert_eq!(info.strength_bar(), "░░░░");
}

#[test]
fn wifi_strength_bar_partial() {
    let info = WifiInfo {
        ssid: "X".into(),
        strength: 50,
        security: WifiSecurity::Wpa2,
        frequency: Some(2412),
        bssid: None,
        mode: WifiMode::Infrastructure,
    };
    assert_eq!(info.strength_bar().chars().count(), 4);
}

#[test]
fn wifi_band_label_2ghz() {
    let info = WifiInfo {
        ssid: "X".into(),
        strength: 70,
        security: WifiSecurity::Wpa2,
        frequency: Some(2412),
        bssid: None,
        mode: WifiMode::Infrastructure,
    };
    assert_eq!(info.band_label(), Some("2.4 GHz"));
}

#[test]
fn wifi_band_label_5ghz() {
    let info = WifiInfo {
        ssid: "X".into(),
        strength: 70,
        security: WifiSecurity::Wpa2,
        frequency: Some(5180),
        bssid: None,
        mode: WifiMode::Infrastructure,
    };
    assert_eq!(info.band_label(), Some("5 GHz"));
}

#[test]
fn wifi_security_labels() {
    assert_eq!(WifiSecurity::None.label(), "Open");
    assert_eq!(WifiSecurity::Wpa2.label(), "WPA2");
    assert_eq!(WifiSecurity::Wpa3.label(), "WPA3");
    assert!(WifiSecurity::Wpa2.is_secured());
    assert!(!WifiSecurity::None.is_secured());
}

#[test]
fn connection_status_indicators() {
    assert_eq!(ConnectionStatus::Active.indicator(), '●');
    assert_eq!(ConnectionStatus::Inactive.indicator(), '○');
}

#[test]
fn nm_state_from_u32() {
    assert_eq!(NmState::from_u32(70), NmState::ConnectedGlobal);
    assert_eq!(NmState::from_u32(20), NmState::Disconnected);
    assert_eq!(NmState::from_u32(99), NmState::Unknown);
}

#[test]
fn nm_state_is_connected() {
    assert!(NmState::ConnectedGlobal.is_connected());
    assert!(NmState::ConnectedLocal.is_connected());
    assert!(!NmState::Disconnected.is_connected());
}

#[test]
fn connection_kind_type_label() {
    assert_eq!(
        ConnectionKind::Wifi(WifiInfo {
            ssid: "x".into(),
            strength: 0,
            security: WifiSecurity::None,
            frequency: None,
            bssid: None,
            mode: WifiMode::Infrastructure,
        })
        .type_label(),
        "Wi-Fi"
    );
    assert_eq!(ConnectionKind::Ethernet.type_label(), "Ethernet");
    assert_eq!(
        ConnectionKind::Vpn(VpnInfo {
            service_type: "openvpn".into()
        })
        .type_label(),
        "VPN"
    );
}
