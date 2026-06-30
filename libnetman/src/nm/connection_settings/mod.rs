// SPDX-License-Identifier: GPL-3.0-only

//! Parse and serialize NetworkManager connection profile settings.

use std::collections::HashMap;

use uuid::Uuid;
use zbus::zvariant::{OwnedValue, Value};

use crate::{
    Result,
    connection::{
        ConnectionProfile, EthernetProfile, IpMethod, Ipv4Profile, Ipv6Profile, VpnProfile,
        WifiProfile, WifiSecurity,
    },
    error::Error,
};

/// NM `Update2` flag: persist changes to disk.
pub const UPDATE2_TO_DISK: u32 = 0x1;

/// Parse a saved connection profile from NM settings (and optional secrets).
pub fn parse_profile(
    raw: &HashMap<String, HashMap<String, OwnedValue>>,
    secrets: Option<&HashMap<String, HashMap<String, OwnedValue>>>,
) -> ConnectionProfile {
    let conn_type = get_str_field(raw, "connection", "type").unwrap_or_default();
    let id = get_str_field(raw, "connection", "id").unwrap_or_else(|| "Unknown".into());

    match conn_type.as_str() {
        "802-11-wireless" => ConnectionProfile::Wifi(parse_wifi(raw, secrets)),
        "802-3-ethernet" => ConnectionProfile::Ethernet(parse_ethernet(raw)),
        "vpn" => ConnectionProfile::Vpn(parse_vpn(raw, secrets)),
        other => ConnectionProfile::Unsupported {
            id,
            conn_type: other.to_owned(),
        },
    }
}

/// Merge editable profile fields into an existing NM settings dict.
pub fn apply_profile(
    raw: &HashMap<String, HashMap<String, OwnedValue>>,
    profile: &ConnectionProfile,
) -> Result<HashMap<String, HashMap<String, OwnedValue>>> {
    let mut settings = raw.clone();

    match profile {
        ConnectionProfile::Wifi(w) => apply_wifi(&mut settings, w, false)?,
        ConnectionProfile::Ethernet(e) => apply_ethernet(&mut settings, e)?,
        ConnectionProfile::Vpn(v) => apply_vpn(&mut settings, v)?,
        ConnectionProfile::Unsupported { .. } => {
            return Err(Error::OperationFailed(
                "this connection type cannot be edited".into(),
            ));
        }
    }

    Ok(settings)
}

/// Build a complete NM settings dict for a new connection profile.
pub fn profile_to_settings(
    profile: &ConnectionProfile,
) -> Result<HashMap<String, HashMap<String, OwnedValue>>> {
    validate_new_profile(profile)?;

    let mut settings: HashMap<String, HashMap<String, OwnedValue>> = HashMap::new();
    let mut connection = HashMap::new();
    connection.insert("uuid".into(), str_value(&Uuid::new_v4().to_string()));

    match profile {
        ConnectionProfile::Wifi(w) => {
            connection.insert("type".into(), str_value("802-11-wireless"));
            connection.insert("id".into(), str_value(&w.ssid));
        }
        ConnectionProfile::Ethernet(e) => {
            connection.insert("type".into(), str_value("802-3-ethernet"));
            connection.insert("id".into(), str_value(e.name.trim()));
        }
        ConnectionProfile::Vpn(v) => {
            connection.insert("type".into(), str_value("vpn"));
            connection.insert("id".into(), str_value(v.name.trim()));
        }
        ConnectionProfile::Unsupported { .. } => {
            return Err(Error::OperationFailed(
                "this connection type cannot be created".into(),
            ));
        }
    }
    settings.insert("connection".into(), connection);

    let mut ipv6 = HashMap::new();
    ipv6.insert("method".into(), str_value("auto"));
    settings.insert("ipv6".into(), ipv6);

    match profile {
        ConnectionProfile::Wifi(w) => apply_wifi(&mut settings, w, true)?,
        ConnectionProfile::Ethernet(e) => apply_ethernet(&mut settings, e)?,
        ConnectionProfile::Vpn(v) => apply_vpn(&mut settings, v)?,
        ConnectionProfile::Unsupported { .. } => unreachable!(),
    }

    Ok(settings)
}

fn validate_new_profile(profile: &ConnectionProfile) -> Result<()> {
    match profile {
        ConnectionProfile::Wifi(w) => {
            if w.ssid.trim().is_empty() {
                return Err(Error::OperationFailed("SSID must not be empty".into()));
            }
            if w.security.is_secured() && w.psk.is_empty() {
                return Err(Error::OperationFailed(
                    "Password is required for secured networks".into(),
                ));
            }
        }
        ConnectionProfile::Ethernet(e) => {
            if e.name.trim().is_empty() {
                return Err(Error::OperationFailed(
                    "Connection name must not be empty".into(),
                ));
            }
        }
        ConnectionProfile::Vpn(v) => {
            if v.name.trim().is_empty() {
                return Err(Error::OperationFailed(
                    "Connection name must not be empty".into(),
                ));
            }
            if v.service_type.trim().is_empty() {
                return Err(Error::OperationFailed(
                    "VPN service type must not be empty".into(),
                ));
            }
        }
        ConnectionProfile::Unsupported { .. } => {
            return Err(Error::OperationFailed(
                "this connection type cannot be created".into(),
            ));
        }
    }
    Ok(())
}

fn parse_wifi(
    raw: &HashMap<String, HashMap<String, OwnedValue>>,
    secrets: Option<&HashMap<String, HashMap<String, OwnedValue>>>,
) -> WifiProfile {
    let ssid = raw
        .get("802-11-wireless")
        .and_then(|s| s.get("ssid"))
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| Vec::<u8>::try_from(v).ok())
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_default();

    let hidden = raw
        .get("802-11-wireless")
        .and_then(|s| get_bool_value(s, "hidden"))
        .unwrap_or(false);

    let security = detect_wifi_security(raw);

    let psk = secrets
        .and_then(|s| s.get("802-11-wireless-security"))
        .and_then(|sec| get_str_value(sec, "psk"))
        .or_else(|| {
            raw.get("802-11-wireless-security")
                .and_then(|sec| get_str_value(sec, "psk"))
        })
        .unwrap_or_default();

    WifiProfile {
        ssid,
        security,
        psk,
        hidden,
        autoconnect: parse_autoconnect(raw),
        vpn_secondary: parse_vpn_secondary(raw),
        ipv4: parse_ipv4(raw),
        ipv6: parse_ipv6(raw),
    }
}

fn parse_ethernet(raw: &HashMap<String, HashMap<String, OwnedValue>>) -> EthernetProfile {
    let name = get_str_field(raw, "connection", "id").unwrap_or_default();
    let eth = raw.get("802-3-ethernet");
    let mtu = eth
        .and_then(|s| get_u32_value(s, "mtu"))
        .map(|m| m.to_string())
        .unwrap_or_default();
    let cloned_mac = eth
        .and_then(|s| get_str_value(s, "cloned-mac-address"))
        .unwrap_or_default();

    EthernetProfile {
        name,
        autoconnect: parse_autoconnect(raw),
        vpn_secondary: parse_vpn_secondary(raw),
        ipv4: parse_ipv4(raw),
        ipv6: parse_ipv6(raw),
        mtu,
        cloned_mac,
    }
}

fn parse_vpn(
    raw: &HashMap<String, HashMap<String, OwnedValue>>,
    secrets: Option<&HashMap<String, HashMap<String, OwnedValue>>>,
) -> VpnProfile {
    let vpn_section = raw.get("vpn");
    let service_type = get_str_field(raw, "vpn", "service-type").unwrap_or_default();
    let gateway = parse_vpn_gateway(vpn_section, &service_type);

    VpnProfile {
        name: get_str_field(raw, "connection", "id").unwrap_or_default(),
        service_type,
        gateway,
        username: vpn_section
            .and_then(|s| get_str_value(s, "username"))
            .unwrap_or_default(),
        password: secrets
            .and_then(|s| s.get("vpn-secrets"))
            .and_then(|sec| get_str_value(sec, "password"))
            .or_else(|| {
                raw.get("vpn-secrets")
                    .and_then(|sec| get_str_value(sec, "password"))
            })
            .unwrap_or_default(),
        port: vpn_section
            .and_then(|s| get_str_value(s, "port"))
            .unwrap_or_default(),
        protocol: vpn_section
            .and_then(|s| get_str_value(s, "proto").or_else(|| get_str_value(s, "protocol")))
            .unwrap_or_else(|| "udp".into()),
        group_name: vpn_section
            .and_then(|s| {
                get_str_value(s, "IPSec-group-name")
                    .or_else(|| get_str_value(s, "ipsec-group-name"))
            })
            .unwrap_or_default(),
        ipv4: parse_ipv4(raw),
        ipv6: parse_ipv6(raw),
    }
}

fn parse_ipv4(raw: &HashMap<String, HashMap<String, OwnedValue>>) -> Ipv4Profile {
    parse_ip_profile(raw.get("ipv4"), 24)
}

fn parse_ipv6(raw: &HashMap<String, HashMap<String, OwnedValue>>) -> Ipv6Profile {
    let parsed = parse_ip_profile(raw.get("ipv6"), 64);
    Ipv6Profile {
        method: parsed.method,
        address: parsed.address,
        prefix: parsed.prefix,
        gateway: parsed.gateway,
        dns: parsed.dns,
    }
}

fn apply_wifi(
    settings: &mut HashMap<String, HashMap<String, OwnedValue>>,
    wifi: &WifiProfile,
    for_new: bool,
) -> Result<()> {
    if wifi.ssid.is_empty() {
        return Err(Error::OperationFailed("SSID must not be empty".into()));
    }
    if matches!(wifi.security, WifiSecurity::Enterprise | WifiSecurity::Wep) {
        return Err(Error::OperationFailed(format!(
            "{} networks cannot be edited here",
            wifi.security.label()
        )));
    }
    if for_new && wifi.security.is_secured() && wifi.psk.is_empty() {
        return Err(Error::OperationFailed(
            "Password is required for secured networks".into(),
        ));
    }

    if let Some(connection) = settings.get_mut("connection") {
        connection.insert("id".into(), str_value(&wifi.ssid));
    }

    let wireless = settings.entry("802-11-wireless".into()).or_default();
    wireless.insert("ssid".into(), bytes_value(wifi.ssid.as_bytes()));
    wireless.insert("mode".into(), str_value("infrastructure"));
    if wifi.hidden {
        wireless.insert("hidden".into(), bool_value(true));
    } else {
        wireless.remove("hidden");
    }

    if wifi.security.is_secured() {
        let sec = settings
            .entry("802-11-wireless-security".into())
            .or_default();
        match wifi.security {
            WifiSecurity::Wpa3 => {
                sec.insert("key-mgmt".into(), str_value("sae"));
            }
            WifiSecurity::Wpa | WifiSecurity::Wpa2 => {
                sec.insert("key-mgmt".into(), str_value("wpa-psk"));
                sec.insert("auth-alg".into(), str_value("open"));
            }
            _ => {}
        }
        if !wifi.psk.is_empty() {
            sec.insert("psk".into(), str_value(&wifi.psk));
        }
    } else {
        settings.remove("802-11-wireless-security");
        settings.remove("802-1x");
    }

    apply_link_options(settings, wifi.autoconnect, wifi.vpn_secondary.as_deref())?;
    apply_ipv4(settings, &wifi.ipv4)?;
    apply_ipv6(settings, &wifi.ipv6)?;
    Ok(())
}

fn apply_ethernet(
    settings: &mut HashMap<String, HashMap<String, OwnedValue>>,
    eth: &EthernetProfile,
) -> Result<()> {
    if let Some(connection) = settings.get_mut("connection")
        && !eth.name.trim().is_empty()
    {
        connection.insert("id".into(), str_value(eth.name.trim()));
    }

    let section = settings.entry("802-3-ethernet".into()).or_default();
    if eth.mtu.trim().is_empty() {
        section.remove("mtu");
    } else {
        let mtu: u32 = eth
            .mtu
            .trim()
            .parse()
            .map_err(|_| Error::OperationFailed("MTU must be a number".into()))?;
        section.insert("mtu".into(), u32_value(mtu));
    }
    if eth.cloned_mac.trim().is_empty() {
        section.remove("cloned-mac-address");
    } else {
        section.insert(
            "cloned-mac-address".into(),
            str_value(eth.cloned_mac.trim()),
        );
    }

    apply_link_options(settings, eth.autoconnect, eth.vpn_secondary.as_deref())?;
    apply_ipv4(settings, &eth.ipv4)?;
    apply_ipv6(settings, &eth.ipv6)?;
    Ok(())
}

fn apply_vpn(
    settings: &mut HashMap<String, HashMap<String, OwnedValue>>,
    vpn: &VpnProfile,
) -> Result<()> {
    if vpn.service_type.trim().is_empty() {
        return Err(Error::OperationFailed(
            "VPN service type must not be empty".into(),
        ));
    }

    if let Some(connection) = settings.get_mut("connection")
        && !vpn.name.trim().is_empty()
    {
        connection.insert("id".into(), str_value(vpn.name.trim()));
    }

    let section = settings.entry("vpn".into()).or_default();
    section.insert("service-type".into(), str_value(vpn.service_type.trim()));

    apply_vpn_gateway(section, &vpn.service_type, vpn.gateway.trim());
    if !vpn.username.trim().is_empty() {
        section.insert("username".into(), str_value(vpn.username.trim()));
    } else {
        section.remove("username");
    }
    if vpn.service_type.contains("openvpn") {
        section.insert("connection-type".into(), str_value("password"));
        if !vpn.port.trim().is_empty() {
            section.insert("port".into(), str_value(vpn.port.trim()));
        } else {
            section.remove("port");
        }
        let proto = if vpn.protocol.eq_ignore_ascii_case("tcp") {
            "tcp"
        } else {
            "udp"
        };
        section.insert("proto".into(), str_value(proto));
    } else {
        section.remove("port");
        section.remove("proto");
        section.remove("protocol");
    }
    if vpn.service_type.contains("vpnc") {
        if !vpn.group_name.trim().is_empty() {
            section.insert("IPSec-group-name".into(), str_value(vpn.group_name.trim()));
        } else {
            section.remove("IPSec-group-name");
            section.remove("ipsec-group-name");
        }
    } else {
        section.remove("IPSec-group-name");
        section.remove("ipsec-group-name");
    }

    if !vpn.password.is_empty() {
        let secrets = settings.entry("vpn-secrets".into()).or_default();
        secrets.insert("password".into(), str_value(&vpn.password));
    } else {
        settings.remove("vpn-secrets");
    }

    apply_ipv4(settings, &vpn.ipv4)?;
    apply_ipv6(settings, &vpn.ipv6)?;
    Ok(())
}

fn apply_ipv4(
    settings: &mut HashMap<String, HashMap<String, OwnedValue>>,
    ipv4: &Ipv4Profile,
) -> Result<()> {
    apply_ip_profile(settings, "ipv4", ipv4, 24)
}

fn apply_ipv6(
    settings: &mut HashMap<String, HashMap<String, OwnedValue>>,
    ipv6: &Ipv6Profile,
) -> Result<()> {
    let ip = Ipv4Profile {
        method: ipv6.method,
        address: ipv6.address.clone(),
        prefix: ipv6.prefix,
        gateway: ipv6.gateway.clone(),
        dns: ipv6.dns.clone(),
    };
    apply_ip_profile(settings, "ipv6", &ip, 64)
}

fn apply_link_options(
    settings: &mut HashMap<String, HashMap<String, OwnedValue>>,
    autoconnect: bool,
    vpn_secondary: Option<&str>,
) -> Result<()> {
    let connection = settings.entry("connection".into()).or_default();
    connection.insert("autoconnect".into(), bool_value(autoconnect));
    if let Some(uuid) = vpn_secondary.filter(|u| !u.is_empty()) {
        connection.insert("secondaries".into(), str_array_value(&[uuid.to_owned()]));
    } else {
        connection.remove("secondaries");
    }
    Ok(())
}

fn parse_autoconnect(raw: &HashMap<String, HashMap<String, OwnedValue>>) -> bool {
    raw.get("connection")
        .and_then(|s| get_bool_value(s, "autoconnect"))
        .unwrap_or(true)
}

fn parse_vpn_secondary(raw: &HashMap<String, HashMap<String, OwnedValue>>) -> Option<String> {
    let values = raw
        .get("connection")
        .and_then(|s| get_str_array(s, "secondaries"))?;
    values.into_iter().find(|u| !u.is_empty())
}

fn parse_ip_profile(
    section: Option<&HashMap<String, OwnedValue>>,
    default_prefix: u32,
) -> Ipv4Profile {
    let method = section
        .and_then(|s| get_str_value(s, "method"))
        .as_deref()
        .map(parse_ip_method)
        .unwrap_or(IpMethod::Auto);

    let mut address = String::new();
    let mut prefix = default_prefix;
    if let Some(sec) = section {
        if let Some(data) = get_address_data(sec, default_prefix) {
            address = data.0;
            prefix = data.1;
        } else if let Some(data) = get_legacy_address(sec) {
            address = data.0;
            prefix = data.1;
        }
    }

    let gateway = section
        .and_then(|s| get_str_value(s, "gateway"))
        .unwrap_or_default();

    let dns = section
        .and_then(|s| get_dns_data(s).or_else(|| get_legacy_dns(s)))
        .unwrap_or_default();

    Ipv4Profile {
        method,
        address,
        prefix,
        gateway,
        dns,
    }
}

fn parse_vpn_gateway(
    vpn_section: Option<&HashMap<String, OwnedValue>>,
    service_type: &str,
) -> String {
    let Some(section) = vpn_section else {
        return String::new();
    };
    for key in vpn_gateway_keys(service_type) {
        if let Some(value) = get_str_value(section, key)
            && !value.is_empty()
        {
            return value;
        }
    }
    String::new()
}

fn vpn_gateway_keys(service_type: &str) -> &'static [&'static str] {
    if service_type.contains("openvpn") {
        &["remote"]
    } else if service_type.contains("openconnect") || service_type.contains("fortisslvpn") {
        &["gateway"]
    } else if service_type.contains("vpnc") {
        &["IPSec gateway", "IPSec-gateway", "gateway"]
    } else if service_type.contains("pptp") || service_type.contains("l2tp") {
        &["gateway"]
    } else if service_type.contains("wireguard") {
        &["endpoint", "remote", "gateway"]
    } else {
        &["gateway", "remote", "IPSec gateway"]
    }
}

fn apply_vpn_gateway(section: &mut HashMap<String, OwnedValue>, service_type: &str, gateway: &str) {
    for key in ALL_VPN_GATEWAY_KEYS {
        section.remove(*key);
    }
    if gateway.is_empty() {
        return;
    }
    let keys = vpn_gateway_keys(service_type);
    if let Some(key) = keys.first() {
        section.insert((*key).into(), str_value(gateway));
    }
}

const ALL_VPN_GATEWAY_KEYS: &[&str] = &[
    "remote",
    "gateway",
    "IPSec gateway",
    "IPSec-gateway",
    "endpoint",
];

fn apply_ip_profile(
    settings: &mut HashMap<String, HashMap<String, OwnedValue>>,
    section_name: &str,
    ip: &Ipv4Profile,
    default_prefix: u32,
) -> Result<()> {
    let section = settings.entry(section_name.into()).or_default();
    section.insert("method".into(), str_value(ip_method_to_nm(ip.method)));

    if ip.method == IpMethod::Manual {
        if ip.address.trim().is_empty() {
            return Err(Error::OperationFailed(format!(
                "Address is required for manual {section_name}"
            )));
        }
        let prefix = if ip.prefix == 0 {
            default_prefix
        } else {
            ip.prefix
        };
        section.insert(
            "address-data".into(),
            address_data_value(&ip.address, prefix),
        );
        section.remove("addresses");
        if !ip.gateway.trim().is_empty() {
            section.insert("gateway".into(), str_value(ip.gateway.trim()));
        } else {
            section.remove("gateway");
        }
        if !ip.dns.trim().is_empty() {
            section.insert("dns-data".into(), dns_data_value(&ip.dns));
            section.remove("dns");
        } else {
            section.remove("dns-data");
            section.remove("dns");
        }
    } else {
        section.remove("address-data");
        section.remove("addresses");
        section.remove("gateway");
        section.remove("dns-data");
        section.remove("dns");
    }

    Ok(())
}

fn detect_wifi_security(raw: &HashMap<String, HashMap<String, OwnedValue>>) -> WifiSecurity {
    if raw.get("802-1x").is_some() {
        return WifiSecurity::Enterprise;
    }
    let key_mgmt = raw
        .get("802-11-wireless-security")
        .and_then(|s| get_str_value(s, "key-mgmt"));

    match key_mgmt.as_deref() {
        Some("wpa-psk") => WifiSecurity::Wpa2,
        Some("sae") => WifiSecurity::Wpa3,
        Some("wpa-eap") => WifiSecurity::Enterprise,
        Some("none") | Some("ieee8021x") => WifiSecurity::Wep,
        _ => WifiSecurity::None,
    }
}

fn parse_ip_method(raw: &str) -> IpMethod {
    match raw {
        "manual" => IpMethod::Manual,
        "disabled" => IpMethod::Disabled,
        "shared" => IpMethod::Shared,
        "link-local" => IpMethod::LinkLocal,
        _ => IpMethod::Auto,
    }
}

fn ip_method_to_nm(method: IpMethod) -> &'static str {
    match method {
        IpMethod::Auto => "auto",
        IpMethod::Manual => "manual",
        IpMethod::Disabled => "disabled",
        IpMethod::Shared => "shared",
        IpMethod::LinkLocal => "link-local",
    }
}

fn get_address_data(
    section: &HashMap<String, OwnedValue>,
    default_prefix: u32,
) -> Option<(String, u32)> {
    let data = section.get("address-data")?;
    let entries: Vec<HashMap<String, OwnedValue>> = Vec::try_from(data.try_clone().ok()?).ok()?;
    let first = entries.first()?;
    let address = get_str_value(first, "address")?;
    let prefix = get_u32_value(first, "prefix").unwrap_or(default_prefix);
    Some((address, prefix))
}

fn get_legacy_address(section: &HashMap<String, OwnedValue>) -> Option<(String, u32)> {
    let data = section.get("addresses")?;
    let entries: Vec<(String, u32)> = Vec::try_from(data.try_clone().ok()?).ok()?;
    let (address, prefix) = entries.first()?.clone();
    Some((address, prefix))
}

fn get_dns_data(section: &HashMap<String, OwnedValue>) -> Option<String> {
    let data = section.get("dns-data")?;
    let entries: Vec<HashMap<String, OwnedValue>> = Vec::try_from(data.try_clone().ok()?).ok()?;
    entries.first().and_then(|e| get_str_value(e, "address"))
}

fn get_legacy_dns(section: &HashMap<String, OwnedValue>) -> Option<String> {
    let data = section.get("dns")?;
    let entries: Vec<u32> = Vec::try_from(data.try_clone().ok()?).ok()?;
    let raw = *entries.first()?;
    let octets = raw.to_be_bytes();
    Some(format!(
        "{}.{}.{}.{}",
        octets[0], octets[1], octets[2], octets[3]
    ))
}

fn get_str_field(
    raw: &HashMap<String, HashMap<String, OwnedValue>>,
    section: &str,
    key: &str,
) -> Option<String> {
    get_str_value(raw.get(section)?, key)
}

fn get_str_value(map: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    let v = map.get(key)?;
    <&str>::try_from(v).ok().map(str::to_owned)
}

fn get_u32_value(map: &HashMap<String, OwnedValue>, key: &str) -> Option<u32> {
    let v = map.get(key)?;
    u32::try_from(v).ok()
}

fn get_bool_value(map: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    let v = map.get(key)?;
    bool::try_from(v).ok()
}

fn get_str_array(map: &HashMap<String, OwnedValue>, key: &str) -> Option<Vec<String>> {
    let v = map.get(key)?;
    Vec::<String>::try_from(v.try_clone().ok()?).ok()
}

fn str_value(s: &str) -> OwnedValue {
    Value::from(s).try_into().expect("string OwnedValue")
}

fn bytes_value(bytes: &[u8]) -> OwnedValue {
    Value::from(bytes.to_vec())
        .try_into()
        .expect("bytes OwnedValue")
}

fn bool_value(value: bool) -> OwnedValue {
    Value::from(value).try_into().expect("bool OwnedValue")
}

fn u32_value(value: u32) -> OwnedValue {
    Value::from(value).try_into().expect("u32 OwnedValue")
}

fn address_data_value(address: &str, prefix: u32) -> OwnedValue {
    let mut entry: HashMap<String, OwnedValue> = HashMap::new();
    entry.insert("address".into(), str_value(address));
    entry.insert("prefix".into(), u32_value(prefix));
    Value::from(vec![entry])
        .try_into()
        .expect("address-data OwnedValue")
}

fn dns_data_value(dns: &str) -> OwnedValue {
    let mut entry: HashMap<String, OwnedValue> = HashMap::new();
    entry.insert("address".into(), str_value(dns));
    Value::from(vec![entry])
        .try_into()
        .expect("dns-data OwnedValue")
}

fn str_array_value(values: &[String]) -> OwnedValue {
    Value::from(values.to_vec())
        .try_into()
        .expect("string array OwnedValue")
}

#[cfg(test)]
mod tests;
