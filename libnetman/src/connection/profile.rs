// SPDX-License-Identifier: GPL-3.0-only

//! Editable connection profile settings (saved NM profiles, not live IP config).

use serde::{Deserialize, Serialize};

use super::WifiSecurity;

/// IPv4 configuration method for a saved connection profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IpMethod {
    Auto,
    Manual,
    Disabled,
    Shared,
    LinkLocal,
}

impl IpMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Automatic (DHCP)",
            Self::Manual => "Manual",
            Self::Disabled => "Disabled",
            Self::Shared => "Shared",
            Self::LinkLocal => "Link-local",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Auto,
            Self::Manual,
            Self::Disabled,
            Self::Shared,
            Self::LinkLocal,
        ]
    }

    pub fn next(self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|m| *m == self).unwrap_or(0);
        all[(idx + 1) % all.len()]
    }

    pub fn prev(self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|m| *m == self).unwrap_or(0);
        all[(idx + all.len() - 1) % all.len()]
    }
}

/// IPv4 settings stored in a connection profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ipv4Profile {
    pub method: IpMethod,
    /// Host address without prefix (e.g. `192.168.1.10`).
    pub address: String,
    /// CIDR prefix length (e.g. `24`).
    pub prefix: u32,
    pub gateway: String,
    /// Primary DNS server address.
    pub dns: String,
}

impl Default for Ipv4Profile {
    fn default() -> Self {
        Self {
            method: IpMethod::Auto,
            address: String::new(),
            prefix: 24,
            gateway: String::new(),
            dns: String::new(),
        }
    }
}

/// IPv6 settings stored in a connection profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ipv6Profile {
    pub method: IpMethod,
    /// Host address without prefix (e.g. `2001:db8::10`).
    pub address: String,
    /// CIDR prefix length (e.g. `64`).
    pub prefix: u32,
    pub gateway: String,
    /// Primary DNS server address.
    pub dns: String,
}

impl Default for Ipv6Profile {
    fn default() -> Self {
        Self {
            method: IpMethod::Auto,
            address: String::new(),
            prefix: 64,
            gateway: String::new(),
            dns: String::new(),
        }
    }
}

/// Editable Wi-Fi profile fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WifiProfile {
    pub ssid: String,
    pub security: WifiSecurity,
    /// Pre-shared key; empty means "leave unchanged" on save.
    pub psk: String,
    pub hidden: bool,
    pub autoconnect: bool,
    /// VPN profile UUID to activate when this connection comes up.
    pub vpn_secondary: Option<String>,
    pub ipv4: Ipv4Profile,
    pub ipv6: Ipv6Profile,
}

/// Editable Ethernet profile fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EthernetProfile {
    /// Connection display name (NM `connection.id`).
    pub name: String,
    pub autoconnect: bool,
    /// VPN profile UUID to activate when this connection comes up.
    pub vpn_secondary: Option<String>,
    pub ipv4: Ipv4Profile,
    pub ipv6: Ipv6Profile,
    /// Empty string means leave MTU unchanged / default.
    pub mtu: String,
    /// Empty string means leave MAC unchanged / default.
    pub cloned_mac: String,
}

/// Editable VPN profile fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VpnProfile {
    /// Connection display name (NM `connection.id`).
    pub name: String,
    pub service_type: String,
    /// VPN gateway / remote host (plugin-specific key).
    pub gateway: String,
    pub username: String,
    /// VPN password; stored in `vpn-secrets` when non-empty.
    pub password: String,
    /// OpenVPN port; empty leaves unchanged.
    pub port: String,
    /// OpenVPN transport (`tcp` / `udp`).
    pub protocol: String,
    /// Cisco VPNC group name.
    pub group_name: String,
    pub ipv4: Ipv4Profile,
    pub ipv6: Ipv6Profile,
}

impl Default for VpnProfile {
    fn default() -> Self {
        Self {
            name: String::new(),
            service_type: String::new(),
            gateway: String::new(),
            username: String::new(),
            password: String::new(),
            port: String::new(),
            protocol: "udp".into(),
            group_name: String::new(),
            ipv4: Ipv4Profile::default(),
            ipv6: Ipv6Profile::default(),
        }
    }
}

/// Top-level editable profile, keyed by connection kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionProfile {
    Wifi(WifiProfile),
    Ethernet(EthernetProfile),
    Vpn(VpnProfile),
    /// Connection types without an editor (loopback, etc.).
    Unsupported {
        id: String,
        conn_type: String,
    },
}

impl ConnectionProfile {
    pub fn is_editable(&self) -> bool {
        !matches!(self, Self::Unsupported { .. })
    }
}
