use crate::connection::WifiSecurity;

use super::wifi_connection_settings;

#[test]
fn open_network_has_no_security_section() {
    let settings = wifi_connection_settings("Cafe", WifiSecurity::None, None).unwrap();
    assert!(settings.contains_key("connection"));
    assert!(settings.contains_key("802-11-wireless"));
    assert!(!settings.contains_key("802-11-wireless-security"));
}

#[test]
fn wpa2_network_requires_password() {
    assert!(wifi_connection_settings("Home", WifiSecurity::Wpa2, None).is_err());
    assert!(wifi_connection_settings("Home", WifiSecurity::Wpa2, Some("")).is_err());
}

#[test]
fn wpa2_network_includes_psk() {
    let settings = wifi_connection_settings("Home", WifiSecurity::Wpa2, Some("secret")).unwrap();
    let sec = settings.get("802-11-wireless-security").unwrap();
    assert_eq!(
        <&str>::try_from(sec.get("key-mgmt").unwrap()).ok(),
        Some("wpa-psk")
    );
    assert_eq!(
        <&str>::try_from(sec.get("psk").unwrap()).ok(),
        Some("secret")
    );
}

#[test]
fn wpa3_uses_sae_key_mgmt() {
    let settings = wifi_connection_settings("Home", WifiSecurity::Wpa3, Some("secret")).unwrap();
    let sec = settings.get("802-11-wireless-security").unwrap();
    assert_eq!(
        <&str>::try_from(sec.get("key-mgmt").unwrap()).ok(),
        Some("sae")
    );
}

#[test]
fn enterprise_is_rejected() {
    assert!(wifi_connection_settings("Corp", WifiSecurity::Enterprise, Some("x")).is_err());
}
