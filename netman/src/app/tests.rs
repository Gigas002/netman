use libnetman::connection::{
    Connection, ConnectionKind, ConnectionStatus, NmState, WifiInfo, WifiMode, WifiSecurity,
};

use super::{ListItem, build_list_items, is_inflight_status};

fn wifi(ssid: &str, strength: u8, active: bool) -> Connection {
    Connection {
        id: ssid.into(),
        uuid: format!("uuid-{ssid}"),
        kind: ConnectionKind::Wifi(WifiInfo {
            ssid: ssid.into(),
            strength,
            security: WifiSecurity::Wpa2,
            frequency: Some(5180),
            bssid: None,
            mode: WifiMode::Infrastructure,
        }),
        status: if active {
            ConnectionStatus::Active
        } else {
            ConnectionStatus::Inactive
        },
        ip4: None,
        device: None,
        saved: true,
    }
}

fn ethernet(id: &str) -> Connection {
    Connection {
        id: id.into(),
        uuid: format!("uuid-{id}"),
        kind: ConnectionKind::Ethernet,
        status: ConnectionStatus::Inactive,
        ip4: None,
        device: None,
        saved: true,
    }
}

#[test]
fn build_list_items_groups_by_type() {
    let conns = vec![
        ethernet("eth0"),
        wifi("Home", 90, true),
        wifi("Cafe", 50, false),
    ];
    let items = build_list_items(conns);

    // First section: Wi-Fi header + 2 connections + hidden entry
    assert!(matches!(&items[0], ListItem::Header(h) if h == "Wi-Fi"));
    assert!(items[1].is_connection());
    assert!(items[2].is_connection());
    assert!(matches!(&items[3], ListItem::HiddenWifiConnect));
    // Second section: Ethernet header + 1 connection
    assert!(matches!(&items[4], ListItem::Header(h) if h == "Ethernet"));
    assert!(items[5].is_connection());
    assert_eq!(items.len(), 6);
}

#[test]
fn build_list_items_empty_input_includes_hidden_entry() {
    let items = build_list_items(vec![]);
    assert_eq!(items.len(), 2);
    assert!(matches!(&items[0], ListItem::Header(h) if h == "Wi-Fi"));
    assert!(matches!(&items[1], ListItem::HiddenWifiConnect));
}

#[test]
fn hidden_wifi_entry_is_selectable() {
    let items = build_list_items(vec![]);
    assert!(items[1].is_selectable());
    assert!(!items[1].is_connection());
}

#[test]
fn nm_state_label_connected_global() {
    assert_eq!(NmState::ConnectedGlobal.label(), "Connected");
}

#[test]
fn connection_status_indicator_active() {
    assert_eq!(ConnectionStatus::Active.indicator(), '●');
}

#[test]
fn inflight_status_messages() {
    assert!(is_inflight_status("Activating…"));
    assert!(is_inflight_status("Deactivating…"));
    assert!(!is_inflight_status("Activation failed: no device"));
    assert!(!is_inflight_status("Demo mode — connect not available"));
}
