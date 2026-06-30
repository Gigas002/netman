// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;

use zbus::zvariant::{OwnedValue, Value};

use crate::connection::{
    ConnectionProfile, EthernetProfile, IpMethod, Ipv4Profile, WifiProfile, WifiSecurity,
};
use super::{apply_profile, parse_profile};

fn str_value(s: &str) -> OwnedValue {
    Value::from(s).try_into().unwrap()
}

fn bytes_value(bytes: &[u8]) -> OwnedValue {
    Value::from(bytes.to_vec()).try_into().unwrap()
}

fn wifi_raw() -> HashMap<String, HashMap<String, OwnedValue>> {
    let mut settings = HashMap::new();

    let mut connection = HashMap::new();
    connection.insert("type".into(), str_value("802-11-wireless"));
    connection.insert("id".into(), str_value("Home"));
    connection.insert("uuid".into(), str_value("test-uuid"));
    settings.insert("connection".into(), connection);

    let mut wireless = HashMap::new();
    wireless.insert("ssid".into(), bytes_value(b"Home"));
    wireless.insert("mode".into(), str_value("infrastructure"));
    settings.insert("802-11-wireless".into(), wireless);

    let mut sec = HashMap::new();
    sec.insert("key-mgmt".into(), str_value("wpa-psk"));
    sec.insert("psk".into(), str_value("secret123"));
    settings.insert("802-11-wireless-security".into(), sec);

    let mut ipv4 = HashMap::new();
    ipv4.insert("method".into(), str_value("auto"));
    settings.insert("ipv4".into(), ipv4);

    settings
}

#[test]
fn parse_wifi_profile() {
    let raw = wifi_raw();
    let profile = parse_profile(&raw, None);
    let ConnectionProfile::Wifi(w) = profile else {
        panic!("expected wifi profile");
    };
    assert_eq!(w.ssid, "Home");
    assert_eq!(w.security, WifiSecurity::Wpa2);
    assert_eq!(w.psk, "secret123");
    assert_eq!(w.ipv4.method, IpMethod::Auto);
}

#[test]
fn apply_wifi_changes_ssid_and_ipv4() {
    let raw = wifi_raw();
    let mut profile = parse_profile(&raw, None);
    let ConnectionProfile::Wifi(ref mut w) = profile else {
        panic!("expected wifi");
    };
    w.ssid = "Office".into();
    w.psk = String::new();
    w.ipv4.method = IpMethod::Manual;
    w.ipv4.address = "10.0.0.5".into();
    w.ipv4.prefix = 24;
    w.ipv4.gateway = "10.0.0.1".into();
    w.ipv4.dns = "1.1.1.1".into();

    let updated = apply_profile(&raw, &profile).unwrap();
    let reparsed = parse_profile(&updated, None);
    let ConnectionProfile::Wifi(w) = reparsed else {
        panic!("expected wifi");
    };
    assert_eq!(w.ssid, "Office");
    assert_eq!(w.ipv4.method, IpMethod::Manual);
    assert_eq!(w.ipv4.address, "10.0.0.5");
    assert_eq!(w.ipv4.gateway, "10.0.0.1");
    assert_eq!(w.ipv4.dns, "1.1.1.1");
}

#[test]
fn parse_ethernet_profile() {
    let mut settings = HashMap::new();
    let mut connection = HashMap::new();
    connection.insert("type".into(), str_value("802-3-ethernet"));
    connection.insert("id".into(), str_value("Wired"));
    settings.insert("connection".into(), connection);

    let mut eth = HashMap::new();
    eth.insert("mtu".into(), Value::from(9000u32).try_into().unwrap());
    settings.insert("802-3-ethernet".into(), eth);

    let mut ipv4 = HashMap::new();
    ipv4.insert("method".into(), str_value("auto"));
    settings.insert("ipv4".into(), ipv4);

    let profile = parse_profile(&settings, None);
    let ConnectionProfile::Ethernet(e) = profile else {
        panic!("expected ethernet");
    };
    assert_eq!(e.mtu, "9000");
    assert_eq!(e.ipv4.method, IpMethod::Auto);
}

#[test]
fn manual_ipv4_requires_address() {
    let raw = wifi_raw();
    let profile = ConnectionProfile::Wifi(WifiProfile {
        ssid: "Home".into(),
        security: WifiSecurity::Wpa2,
        psk: String::new(),
        hidden: false,
        ipv4: Ipv4Profile {
            method: IpMethod::Manual,
            ..Ipv4Profile::default()
        },
    });
    assert!(apply_profile(&raw, &profile).is_err());
}

#[test]
fn ethernet_round_trip_mtu() {
    let mut settings = HashMap::new();
    let mut connection = HashMap::new();
    connection.insert("type".into(), str_value("802-3-ethernet"));
    connection.insert("id".into(), str_value("Wired"));
    settings.insert("connection".into(), connection);
    settings.insert("802-3-ethernet".into(), HashMap::new());
    settings.insert(
        "ipv4".into(),
        HashMap::from([("method".into(), str_value("auto"))]),
    );

    let profile = ConnectionProfile::Ethernet(EthernetProfile {
        ipv4: Ipv4Profile::default(),
        mtu: "1500".into(),
        cloned_mac: String::new(),
    });
    let updated = apply_profile(&settings, &profile).unwrap();
    let reparsed = parse_profile(&updated, None);
    let ConnectionProfile::Ethernet(e) = reparsed else {
        panic!("expected ethernet");
    };
    assert_eq!(e.mtu, "1500");
}
