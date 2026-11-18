use libnetman::connection::{
    Connection, ConnectionKind, ConnectionProfile, ConnectionStatus, EthernetProfile, Ipv4Profile,
    Ipv6Profile, NmState, VpnProfile, WifiInfo, WifiMode, WifiProfile, WifiSecurity,
};

use super::{EditorFieldId, ListItem, build_list_items, editor_fields_for, is_inflight_status};

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
        ip6: None,
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
        ip6: None,
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

#[test]
fn editor_fields_vary_by_connection_type() {
    let wifi = editor_fields_for(
        &ConnectionProfile::Wifi(WifiProfile {
            ssid: "x".into(),
            security: WifiSecurity::Wpa2,
            psk: String::new(),
            hidden: false,
            autoconnect: true,
            vpn_secondary: None,
            ipv4: Ipv4Profile::default(),
            ipv6: Ipv6Profile::default(),
        }),
        false,
    );
    assert!(wifi.contains(&EditorFieldId::Ssid));
    assert!(!wifi.contains(&EditorFieldId::Mtu));

    let eth = editor_fields_for(
        &ConnectionProfile::Ethernet(EthernetProfile {
            name: "eth".into(),
            autoconnect: true,
            vpn_secondary: None,
            ipv4: Ipv4Profile::default(),
            ipv6: Ipv6Profile::default(),
            mtu: String::new(),
            cloned_mac: String::new(),
        }),
        false,
    );
    assert!(eth.contains(&EditorFieldId::ConnectionName));
    assert!(!eth.contains(&EditorFieldId::Activate));

    let eth_new = editor_fields_for(
        &ConnectionProfile::Ethernet(EthernetProfile {
            name: "eth".into(),
            autoconnect: true,
            vpn_secondary: None,
            ipv4: Ipv4Profile::default(),
            ipv6: Ipv6Profile::default(),
            mtu: String::new(),
            cloned_mac: String::new(),
        }),
        true,
    );
    assert!(eth_new.contains(&EditorFieldId::Activate));

    let vpn = editor_fields_for(
        &ConnectionProfile::Vpn(VpnProfile {
            name: "vpn".into(),
            service_type: "org.freedesktop.NetworkManager.openvpn".into(),
            ..VpnProfile::default()
        }),
        false,
    );
    assert!(vpn.contains(&EditorFieldId::VpnGateway));
    assert!(vpn.contains(&EditorFieldId::VpnPort));
    assert!(vpn.contains(&EditorFieldId::VpnServiceType));
}

#[cfg(feature = "mobile")]
#[test]
fn build_list_items_includes_mobile_section() {
    use libnetman::connection::{AccessTechnology, ModemInfo};

    let conns = vec![Connection {
        id: "LTE".into(),
        uuid: "uuid-lte".into(),
        kind: ConnectionKind::Modem(ModemInfo {
            apn: Some("internet".into()),
            operator_name: Some("Carrier".into()),
            operator_code: None,
            signal_quality: 80,
            access_technology: AccessTechnology::Lte,
            sim_locked: false,
        }),
        status: ConnectionStatus::Inactive,
        ip4: None,
        ip6: None,
        device: Some("wwan0".into()),
        saved: true,
    }];

    let items = build_list_items(conns);
    assert!(
        items
            .iter()
            .any(|i| matches!(i, ListItem::Header(h) if h == "Mobile"))
    );
}

// ── App behavior ──────────────────────────────────────────────────────────────

/// An `App` showing `connections`, as if NetworkManager had returned them.
fn app_with(connections: Vec<Connection>) -> super::App {
    super::App {
        items: build_list_items(connections),
        selected: 0,
        show_detail: true,
        nm_state: NmState::ConnectedGlobal,
        networking_enabled: true,
        wireless_enabled: true,
        demo_mode: false,
        show_help: false,
        status_message: None,
        password_prompt: None,
        hidden_network_prompt: None,
        connection_editor: None,
        add_connection_menu: None,
        vpn_add_menu: None,
        vpn_import_prompt: None,
        delete_confirm_prompt: None,
        #[cfg(feature = "dbus")]
        scan_pending: None,
        #[cfg(feature = "mobile")]
        pin_unlock_prompt: None,
        #[cfg(feature = "dbus")]
        nm: None,
        state_watcher: {
            #[cfg(feature = "dbus")]
            {
                super::StateChangeWaiter::new(None)
            }
            #[cfg(not(feature = "dbus"))]
            {
                super::StateChangeWaiter::new()
            }
        },
    }
}

fn visible(ssid: &str, strength: u8, security: WifiSecurity) -> Connection {
    let mut c = wifi(ssid, strength, false);
    c.uuid = format!("visible:{ssid}");
    c.saved = false;
    if let ConnectionKind::Wifi(w) = &mut c.kind {
        w.security = security;
    }
    c
}

#[test]
fn enter_on_an_owe_network_connects_without_a_password() {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    let mut app = app_with(vec![visible("Enhanced-Open", 70, WifiSecurity::Owe)]);

    let action = app.handle_key(KeyCode::Enter, KeyModifiers::NONE);

    assert!(app.password_prompt.is_none());
    assert!(matches!(
        action,
        super::Action::ConnectUnsaved {
            security: WifiSecurity::Owe,
            password: None,
            ..
        }
    ));
}

#[test]
fn selection_follows_the_connection_when_a_refresh_reorders_the_list() {
    let mut app = app_with(vec![wifi("Strong", 90, false), wifi("Weak", 40, false)]);
    app.selected = 1;
    assert_eq!(app.selected_conn().unwrap().id, "Weak");

    // Signal changed: "Weak" now sorts first.
    app.set_items(build_list_items(vec![
        wifi("Strong", 30, false),
        wifi("Weak", 80, false),
    ]));

    assert_eq!(app.selected_conn().unwrap().id, "Weak");
}

#[test]
fn selection_stays_in_range_when_the_selected_connection_disappears() {
    let mut app = app_with(vec![
        wifi("A", 90, false),
        wifi("B", 80, false),
        wifi("C", 70, false),
    ]);
    app.selected = 3; // the "Connect to hidden…" row
    app.set_items(build_list_items(vec![wifi("A", 90, false)]));
    assert!(app.selected_hidden_wifi());

    app.selected = 0;
    app.set_items(build_list_items(vec![wifi("B", 80, false)]));
    assert_eq!(app.selected_conn().unwrap().id, "B");
}

#[cfg(feature = "dbus")]
#[test]
fn scanning_status_clears_when_last_scan_moves() {
    let mut app = app_with(vec![]);
    app.status_message = Some(super::SCANNING.into());
    app.scan_pending = Some(5_000);

    app.note_last_scan(Some(5_000));
    assert_eq!(app.status_message.as_deref(), Some(super::SCANNING));

    app.note_last_scan(Some(9_000));
    assert_eq!(app.status_message, None);
    assert_eq!(app.scan_pending, None);
}

#[cfg(feature = "dbus")]
#[test]
fn scanning_status_clears_when_last_scan_is_unavailable() {
    let mut app = app_with(vec![]);
    app.status_message = Some(super::SCANNING.into());
    app.scan_pending = Some(-1);

    app.note_last_scan(None);

    assert_eq!(app.status_message, None);
}

#[cfg(feature = "dbus")]
#[test]
fn auto_scan_only_with_nm_and_wifi_on() {
    let mut app = app_with(vec![]);
    assert!(app.wants_auto_scan());
    app.wireless_enabled = false;
    assert!(!app.wants_auto_scan());
    app.wireless_enabled = true;
    app.networking_enabled = false;
    assert!(!app.wants_auto_scan());
    app.networking_enabled = true;
    app.demo_mode = true;
    assert!(!app.wants_auto_scan());
}

#[test]
fn auto_scan_timer_follows_settings() {
    let mut settings = crate::settings::Settings::default();
    assert!(settings.auto_scan);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let _guard = runtime.enter();
    assert_eq!(
        super::auto_scan_timer(&settings).map(|t| t.period()),
        Some(std::time::Duration::from_secs(30))
    );
    settings.scan_interval = 0;
    assert!(super::auto_scan_timer(&settings).is_none());
    settings.scan_interval = 30;
    settings.auto_scan = false;
    assert!(super::auto_scan_timer(&settings).is_none());
}
