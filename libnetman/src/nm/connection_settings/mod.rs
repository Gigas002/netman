// SPDX-License-Identifier: GPL-3.0-only

//! Parse and serialize NetworkManager connection profile settings.

use std::collections::HashMap;

use zbus::zvariant::{OwnedValue, Value};

use crate::{
    Result,
    connection::{
        ConnectionProfile, EthernetProfile, IpMethod, Ipv4Profile, VpnProfile, WifiProfile,
        WifiSecurity,
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
        "vpn" => ConnectionProfile::Vpn(parse_vpn(raw)),
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
        ConnectionProfile::Wifi(w) => apply_wifi(&mut settings, w)?,
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
        ipv4: parse_ipv4(raw),
    }
}

fn parse_ethernet(raw: &HashMap<String, HashMap<String, OwnedValue>>) -> EthernetProfile {
    let eth = raw.get("802-3-ethernet");
    let mtu = eth
        .and_then(|s| get_u32_value(s, "mtu"))
        .map(|m| m.to_string())
        .unwrap_or_default();
    let cloned_mac = eth
        .and_then(|s| get_str_value(s, "cloned-mac-address"))
        .unwrap_or_default();

    EthernetProfile {
        ipv4: parse_ipv4(raw),
        mtu,
        cloned_mac,
    }
}

fn parse_vpn(raw: &HashMap<String, HashMap<String, OwnedValue>>) -> VpnProfile {
    VpnProfile {
        service_type: get_str_field(raw, "vpn", "service-type").unwrap_or_default(),
        ipv4: parse_ipv4(raw),
    }
}

fn parse_ipv4(raw: &HashMap<String, HashMap<String, OwnedValue>>) -> Ipv4Profile {
    let section = raw.get("ipv4");
    let method = section
        .and_then(|s| get_str_value(s, "method"))
        .as_deref()
        .map(parse_ip_method)
        .unwrap_or(IpMethod::Auto);

    let mut address = String::new();
    let mut prefix = 24u32;
    if let Some(sec) = section {
        if let Some(data) = get_address_data(sec) {
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

fn apply_wifi(
    settings: &mut HashMap<String, HashMap<String, OwnedValue>>,
    wifi: &WifiProfile,
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
    if wifi.security.is_secured() && wifi.psk.is_empty() {
        // Allow empty PSK when updating other fields — only validate on explicit change
        // by checking if security section exists; NM keeps existing secret.
    }

    let wireless = settings
        .entry("802-11-wireless".into())
        .or_default();
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

    apply_ipv4(settings, &wifi.ipv4)?;
    Ok(())
}

fn apply_ethernet(
    settings: &mut HashMap<String, HashMap<String, OwnedValue>>,
    eth: &EthernetProfile,
) -> Result<()> {
    let section = settings
        .entry("802-3-ethernet".into())
        .or_default();
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

    apply_ipv4(settings, &eth.ipv4)?;
    Ok(())
}

fn apply_vpn(
    settings: &mut HashMap<String, HashMap<String, OwnedValue>>,
    vpn: &VpnProfile,
) -> Result<()> {
    apply_ipv4(settings, &vpn.ipv4)?;
    let _ = vpn.service_type;
    Ok(())
}

fn apply_ipv4(
    settings: &mut HashMap<String, HashMap<String, OwnedValue>>,
    ipv4: &Ipv4Profile,
) -> Result<()> {
    let section = settings.entry("ipv4".into()).or_default();
    section.insert("method".into(), str_value(ip_method_to_nm(ipv4.method)));

    if ipv4.method == IpMethod::Manual {
        if ipv4.address.trim().is_empty() {
            return Err(Error::OperationFailed(
                "Address is required for manual IPv4".into(),
            ));
        }
        let prefix = if ipv4.prefix == 0 { 24 } else { ipv4.prefix };
        section.insert(
            "address-data".into(),
            address_data_value(&ipv4.address, prefix),
        );
        section.remove("addresses");
        if !ipv4.gateway.trim().is_empty() {
            section.insert("gateway".into(), str_value(ipv4.gateway.trim()));
        } else {
            section.remove("gateway");
        }
        if !ipv4.dns.trim().is_empty() {
            section.insert("dns-data".into(), dns_data_value(&ipv4.dns));
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

fn get_address_data(section: &HashMap<String, OwnedValue>) -> Option<(String, u32)> {
    let data = section.get("address-data")?;
    let entries: Vec<HashMap<String, OwnedValue>> = Vec::try_from(data.try_clone().ok()?).ok()?;
    let first = entries.first()?;
    let address = get_str_value(first, "address")?;
    let prefix = get_u32_value(first, "prefix").unwrap_or(24);
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
    entries
        .first()
        .and_then(|e| get_str_value(e, "address"))
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

#[cfg(test)]
mod tests;
