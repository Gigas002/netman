// SPDX-License-Identifier: GPL-3.0-only

use std::io::Write;

use tempfile::TempDir;

use super::parse_import_uuid;

#[test]
fn parse_import_uuid_from_nmcli_output() {
    let stdout =
        "Connection 'Work VPN' (65d9e7f2-3c1a-4b2e-9f0a-1234567890ab) successfully added.\n";
    assert_eq!(
        parse_import_uuid(stdout),
        Some("65d9e7f2-3c1a-4b2e-9f0a-1234567890ab".into())
    );
}

#[test]
fn list_plugins_from_temp_dir() {
    let dir = TempDir::new().unwrap();
    let vpn_dir = dir.path().join("VPN");
    std::fs::create_dir(&vpn_dir).unwrap();

    let mut file = std::fs::File::create(vpn_dir.join("nm-openvpn-service.name")).unwrap();
    writeln!(
        file,
        "[VPN Connection]\nname=openvpn\nservice=org.freedesktop.NetworkManager.openvpn\n"
    )
    .unwrap();

    let plugin = super::parse_name_file(&vpn_dir.join("nm-openvpn-service.name")).unwrap();
    assert_eq!(plugin.name, "openvpn");
    assert_eq!(
        plugin.service_type,
        "org.freedesktop.NetworkManager.openvpn"
    );
    assert_eq!(plugin.label, "OpenVPN");
}
