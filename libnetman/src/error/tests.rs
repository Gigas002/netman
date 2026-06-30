use super::*;

#[test]
fn error_display_dbus() {
    let e = Error::DBus("connection refused".into());
    assert!(e.to_string().contains("D-Bus error"));
}

#[test]
fn error_display_nm_unavailable() {
    let e = Error::NmUnavailable;
    assert!(e.to_string().contains("NetworkManager"));
}

#[test]
fn error_display_device_not_found() {
    let e = Error::DeviceNotFound("wlan0".into());
    assert!(e.to_string().contains("wlan0"));
}

#[test]
fn error_display_connection_not_found() {
    let e = Error::ConnectionNotFound("Home WiFi".into());
    assert!(e.to_string().contains("Home WiFi"));
}

#[test]
fn error_display_operation_failed() {
    let e = Error::OperationFailed("timeout".into());
    assert!(e.to_string().contains("timeout"));
}
