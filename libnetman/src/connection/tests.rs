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
        ip6: None,
        device: Some("wlan0".into()),
        saved: true,
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

#[test]
fn merge_wifi_scan_data_updates_saved_and_adds_visible() {
    let mut connections = vec![wifi_connection(
        0,
        WifiSecurity::Wpa2,
        ConnectionStatus::Inactive,
    )];

    merge_wifi_scan_data(
        &mut connections,
        vec![
            WifiInfo {
                ssid: "TestNet".into(),
                strength: 72,
                security: WifiSecurity::Wpa3,
                frequency: Some(2437),
                bssid: Some("aa:bb:cc:dd:ee:01".into()),
                mode: WifiMode::Infrastructure,
            },
            WifiInfo {
                ssid: "OpenCafe".into(),
                strength: 55,
                security: WifiSecurity::None,
                frequency: Some(5240),
                bssid: None,
                mode: WifiMode::Infrastructure,
            },
        ],
    );

    let wifi = &connections[0];
    let ConnectionKind::Wifi(info) = &wifi.kind else {
        panic!("expected wifi");
    };
    assert_eq!(info.strength, 72);
    assert_eq!(info.security, WifiSecurity::Wpa3);
    assert!(
        connections
            .iter()
            .any(|c| !c.saved && c.label() == "OpenCafe")
    );
}

#[cfg(feature = "mobile")]
#[test]
fn modem_strength_bar_full() {
    use super::{AccessTechnology, ModemInfo};

    let info = ModemInfo {
        apn: None,
        operator_name: None,
        operator_code: None,
        signal_quality: 100,
        access_technology: AccessTechnology::Lte,
        sim_locked: false,
    };
    assert_eq!(info.strength_bar(), "████");
}

#[cfg(feature = "mobile")]
#[test]
fn merge_modem_live_data_updates_matching_device() {
    use super::{AccessTechnology, ConnectionKind, ModemInfo, ModemLiveData};

    let mut connections = vec![Connection {
        id: "Mobile".into(),
        uuid: "uuid-mobile".into(),
        kind: ConnectionKind::Modem(ModemInfo {
            apn: Some("old".into()),
            operator_name: None,
            operator_code: None,
            signal_quality: 0,
            access_technology: AccessTechnology::Unknown,
            sim_locked: false,
        }),
        status: ConnectionStatus::Inactive,
        ip4: None,
        ip6: None,
        device: Some("wwan0".into()),
        saved: true,
    }];

    merge_modem_live_data(
        &mut connections,
        &[ModemLiveData {
            interface: "wwan0".into(),
            apn: Some("internet".into()),
            operator_name: Some("Carrier".into()),
            operator_code: Some("00101".into()),
            signal_quality: 65,
            access_technology: AccessTechnology::Lte,
            sim_locked: true,
        }],
    );

    let ConnectionKind::Modem(m) = &connections[0].kind else {
        panic!("expected modem");
    };
    assert_eq!(m.apn.as_deref(), Some("internet"));
    assert_eq!(m.operator_name.as_deref(), Some("Carrier"));
    assert_eq!(m.signal_quality, 65);
    assert!(m.sim_locked);
}
