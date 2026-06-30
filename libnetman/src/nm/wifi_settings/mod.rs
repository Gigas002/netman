// SPDX-License-Identifier: GPL-3.0-only

//! Helpers for building NetworkManager Wi-Fi connection settings dicts.

use std::collections::HashMap;

use uuid::Uuid;
use zbus::zvariant::{OwnedValue, Value};

use crate::{Result, connection::WifiSecurity, error::Error};

/// Build a connection settings dict suitable for `AddAndActivateConnection`.
pub fn wifi_connection_settings(
    ssid: &str,
    security: WifiSecurity,
    password: Option<&str>,
    hidden: bool,
) -> Result<HashMap<String, HashMap<String, OwnedValue>>> {
    if ssid.is_empty() {
        return Err(Error::OperationFailed("SSID must not be empty".into()));
    }

    match security {
        WifiSecurity::Enterprise => {
            return Err(Error::OperationFailed(
                "802.1X enterprise networks are not supported here".into(),
            ));
        }
        WifiSecurity::Wep => {
            return Err(Error::OperationFailed(
                "WEP networks are not supported here".into(),
            ));
        }
        WifiSecurity::None => {}
        WifiSecurity::Wpa | WifiSecurity::Wpa2 | WifiSecurity::Wpa3 => {
            if password.is_none_or(str::is_empty) {
                return Err(Error::OperationFailed(
                    "Password is required for secured networks".into(),
                ));
            }
        }
    }

    let mut settings: HashMap<String, HashMap<String, OwnedValue>> = HashMap::new();

    let mut connection = HashMap::new();
    connection.insert("type".into(), str_value("802-11-wireless"));
    connection.insert("uuid".into(), str_value(&Uuid::new_v4().to_string()));
    connection.insert("id".into(), str_value(ssid));
    settings.insert("connection".into(), connection);

    let mut wireless = HashMap::new();
    wireless.insert("ssid".into(), bytes_value(ssid.as_bytes()));
    wireless.insert("mode".into(), str_value("infrastructure"));
    if hidden {
        wireless.insert("hidden".into(), bool_value(true));
    }
    settings.insert("802-11-wireless".into(), wireless);

    if security.is_secured() {
        let psk = password.expect("validated above");
        let mut sec = HashMap::new();
        match security {
            WifiSecurity::Wpa3 => {
                sec.insert("key-mgmt".into(), str_value("sae"));
            }
            WifiSecurity::Wpa | WifiSecurity::Wpa2 => {
                sec.insert("key-mgmt".into(), str_value("wpa-psk"));
                sec.insert("auth-alg".into(), str_value("open"));
            }
            _ => unreachable!(),
        }
        sec.insert("psk".into(), str_value(psk));
        settings.insert("802-11-wireless-security".into(), sec);
    }

    let mut ipv4 = HashMap::new();
    ipv4.insert("method".into(), str_value("auto"));
    settings.insert("ipv4".into(), ipv4);

    let mut ipv6 = HashMap::new();
    ipv6.insert("method".into(), str_value("ignore"));
    settings.insert("ipv6".into(), ipv6);

    Ok(settings)
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

#[cfg(test)]
mod tests;
