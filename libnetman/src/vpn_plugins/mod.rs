// SPDX-License-Identifier: GPL-3.0-only

//! Discovery of installed NetworkManager VPN plugins.

use std::path::Path;

/// An installed VPN plugin described by a `*.name` file under `NetworkManager/VPN`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VpnPluginInfo {
    /// Short plugin id used by `nmcli connection import type <name>`.
    pub name: String,
    /// D-Bus VPN service type (e.g. `org.freedesktop.NetworkManager.openvpn`).
    pub service_type: String,
    /// Human-readable label for UI menus.
    pub label: String,
}

/// Standard directories containing NM VPN plugin descriptors.
const VPN_PLUGIN_DIRS: &[&str] = &[
    "/usr/lib/NetworkManager/VPN",
    "/usr/lib64/NetworkManager/VPN",
    "/usr/local/lib/NetworkManager/VPN",
    "/etc/NetworkManager/VPN",
];

/// Return installed VPN plugins, de-duplicated by service type.
pub fn list_installed_plugins() -> Vec<VpnPluginInfo> {
    let mut plugins = Vec::new();

    for dir in VPN_PLUGIN_DIRS {
        let path = Path::new(dir);
        if !path.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(path) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "name")
                && let Some(plugin) = parse_name_file_impl(&path)
            {
                plugins.push(plugin);
            }
        }
    }

    plugins.sort_by(|a, b| a.label.cmp(&b.label));
    plugins.dedup_by(|a, b| a.service_type == b.service_type);
    plugins
}

#[cfg(test)]
pub(crate) fn parse_name_file(path: &Path) -> Option<VpnPluginInfo> {
    parse_name_file_impl(path)
}

fn parse_name_file_impl(path: &Path) -> Option<VpnPluginInfo> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut name = None;
    let mut service_type = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "name" => name = Some(value.trim().to_owned()),
            "service" => service_type = Some(value.trim().to_owned()),
            _ => {}
        }
    }

    let name = name?;
    let service_type = service_type?;
    let label = humanize_plugin_name(&name);
    Some(VpnPluginInfo {
        name,
        service_type,
        label,
    })
}

fn humanize_plugin_name(name: &str) -> String {
    match name {
        "openvpn" => "OpenVPN".into(),
        "wireguard" => "WireGuard".into(),
        "vpnc" => "VPNC".into(),
        "openconnect" => "OpenConnect".into(),
        "pptp" => "PPTP".into(),
        "l2tp" => "L2TP".into(),
        "iodine" => "Iodine".into(),
        "libreswan" => "Libreswan".into(),
        "fortisslvpn" => "Fortinet SSL VPN".into(),
        "sstp" => "SSTP".into(),
        other => {
            if other.is_empty() {
                "VPN".into()
            } else {
                let mut chars = other.chars();
                let first = chars.next().unwrap().to_uppercase().collect::<String>();
                format!("{first}{}", chars.as_str())
            }
        }
    }
}

/// Parse a connection UUID from `nmcli connection import` stdout.
pub fn parse_import_uuid(stdout: &str) -> Option<String> {
    // e.g. Connection 'My VPN' (65d9e7f2-3c1a-4b2e-9f0a-1234567890ab) successfully added.
    let start = stdout.find('(')? + 1;
    let rest = &stdout[start..];
    let end = rest.find(')')?;
    let uuid = rest[..end].trim();
    if uuid.len() >= 36 && uuid.contains('-') {
        Some(uuid.to_owned())
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
