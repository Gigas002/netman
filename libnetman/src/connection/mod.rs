// SPDX-License-Identifier: GPL-3.0-only

//! Domain types for network connections, devices, and access points.
//!
//! These types mirror the data exposed by NetworkManager but are decoupled
//! from D-Bus specifics so the rest of the application can be tested without a
//! running NM daemon.

use serde::{Deserialize, Serialize};

pub mod profile;

pub use profile::{
    ConnectionProfile, EthernetProfile, IpMethod, Ipv4Profile, VpnProfile, WifiProfile,
};

// ── Connection ────────────────────────────────────────────────────────────────

/// A network connection known to NetworkManager (saved or active).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    /// Human-readable connection name (from NM connection profile).
    pub id: String,
    /// Unique identifier assigned by NetworkManager.
    pub uuid: String,
    /// Connection kind and its type-specific data.
    pub kind: ConnectionKind,
    /// Current lifecycle state.
    pub status: ConnectionStatus,
    /// Network layer details; populated only for active connections.
    pub ip4: Option<Ip4Config>,
    /// Interface name of the attached device (e.g. `wlan0`, `eth0`).
    pub device: Option<String>,
    /// Whether this entry is a saved NM profile (`true`) or a visible-only AP (`false`).
    pub saved: bool,
}

impl Connection {
    /// Returns `true` if the connection is currently active.
    pub fn is_active(&self) -> bool {
        matches!(self.status, ConnectionStatus::Active)
    }

    /// Returns `true` if this entry is a saved connection profile.
    pub fn is_saved(&self) -> bool {
        self.saved
    }

    /// Returns a short label suitable for UI list display.
    pub fn label(&self) -> &str {
        match &self.kind {
            ConnectionKind::Wifi(w) => &w.ssid,
            _ => &self.id,
        }
    }
}

// ── ConnectionKind ────────────────────────────────────────────────────────────

/// The type-specific data for a connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConnectionKind {
    Wifi(WifiInfo),
    Ethernet,
    Vpn(VpnInfo),
    Loopback,
    Other(String),
}

impl ConnectionKind {
    /// Short type label used for section headers and detail views.
    pub fn type_label(&self) -> &str {
        match self {
            Self::Wifi(_) => "Wi-Fi",
            Self::Ethernet => "Ethernet",
            Self::Vpn(_) => "VPN",
            Self::Loopback => "Loopback",
            Self::Other(t) => t.as_str(),
        }
    }
}

// ── WifiInfo ──────────────────────────────────────────────────────────────────

/// Wi-Fi–specific connection data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WifiInfo {
    /// SSID as a UTF-8 string.
    pub ssid: String,
    /// Signal strength 0–100.
    pub strength: u8,
    /// Security mode.
    pub security: WifiSecurity,
    /// Operating frequency in MHz (e.g. 2412 for 2.4 GHz channel 1).
    pub frequency: Option<u32>,
    /// Access-point hardware address (BSSID).
    pub bssid: Option<String>,
    /// Wi-Fi mode (infrastructure, ad-hoc, …).
    pub mode: WifiMode,
}

impl WifiInfo {
    /// Renders signal strength as a bar: `▁▂▃▄▅▆▇█` (4-character wide).
    pub fn strength_bar(&self) -> String {
        let bars = (self.strength as usize * 4 / 100).min(4);
        let filled = "█".repeat(bars);
        let empty = "░".repeat(4 - bars);
        format!("{filled}{empty}")
    }

    /// Frequency band label: "2.4 GHz" or "5 GHz".
    pub fn band_label(&self) -> Option<&'static str> {
        self.frequency
            .map(|f| if f < 3000 { "2.4 GHz" } else { "5 GHz" })
    }
}

/// Wi-Fi security modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WifiSecurity {
    None,
    Wep,
    Wpa,
    Wpa2,
    Wpa3,
    Enterprise,
}

impl WifiSecurity {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Open",
            Self::Wep => "WEP",
            Self::Wpa => "WPA",
            Self::Wpa2 => "WPA2",
            Self::Wpa3 => "WPA3",
            Self::Enterprise => "802.1X",
        }
    }

    pub fn is_secured(self) -> bool {
        !matches!(self, Self::None)
    }

    /// Security modes the connection editor allows cycling through.
    pub fn editable_values() -> &'static [Self] {
        &[Self::None, Self::Wpa2, Self::Wpa3]
    }

    pub fn next_editable(self) -> Self {
        let all = Self::editable_values();
        let idx = all.iter().position(|s| *s == self).unwrap_or(0);
        all[(idx + 1) % all.len()]
    }

    pub fn prev_editable(self) -> Self {
        let all = Self::editable_values();
        let idx = all.iter().position(|s| *s == self).unwrap_or(0);
        all[(idx + all.len() - 1) % all.len()]
    }
}

/// Wi-Fi operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WifiMode {
    Infrastructure,
    AdHoc,
    Ap,
    Mesh,
    Unknown,
}

// ── VpnInfo ───────────────────────────────────────────────────────────────────

/// VPN-specific connection data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VpnInfo {
    /// VPN service type (e.g. `org.freedesktop.NetworkManager.openvpn`).
    pub service_type: String,
}

// ── ConnectionStatus ──────────────────────────────────────────────────────────

/// Lifecycle state of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionStatus {
    /// Connection is fully established.
    Active,
    /// Connection is in the process of being established.
    Activating,
    /// Connection is being deactivated.
    Deactivating,
    /// Connection is saved but not currently active.
    Inactive,
    /// State cannot be determined.
    Unknown,
}

impl ConnectionStatus {
    pub fn indicator(self) -> char {
        match self {
            Self::Active => '●',
            Self::Activating | Self::Deactivating => '◌',
            Self::Inactive => '○',
            Self::Unknown => '?',
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Activating => "Activating…",
            Self::Deactivating => "Deactivating…",
            Self::Inactive => "Inactive",
            Self::Unknown => "Unknown",
        }
    }
}

// ── Ip4Config ─────────────────────────────────────────────────────────────────

/// IPv4 network configuration for an active connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ip4Config {
    /// Address in CIDR notation (e.g. `192.168.1.100/24`).
    pub address: String,
    /// Default gateway address.
    pub gateway: Option<String>,
    /// DNS server addresses.
    pub nameservers: Vec<String>,
}

/// Merge live scan data into saved Wi-Fi connections and append visible-only APs.
///
/// For each saved Wi-Fi profile whose SSID appears in `access_points`, live
/// signal / frequency / BSSID / security data replaces the stale profile values.
/// Access points with no matching saved profile are appended as unsaved entries.
pub fn merge_wifi_scan_data(connections: &mut Vec<Connection>, access_points: Vec<WifiInfo>) {
    use std::collections::{HashMap, HashSet};

    // Keep the strongest AP per SSID.
    let mut best_by_ssid: HashMap<String, WifiInfo> = HashMap::new();
    for ap in access_points {
        best_by_ssid
            .entry(ap.ssid.clone())
            .and_modify(|existing| {
                if ap.strength > existing.strength {
                    *existing = ap.clone();
                }
            })
            .or_insert(ap);
    }

    let mut saved_ssids = HashSet::new();

    for conn in connections.iter_mut() {
        let ConnectionKind::Wifi(wifi) = &mut conn.kind else {
            continue;
        };
        saved_ssids.insert(wifi.ssid.clone());
        if let Some(live) = best_by_ssid.get(&wifi.ssid) {
            wifi.strength = live.strength;
            wifi.frequency = live.frequency;
            wifi.bssid = live.bssid.clone();
            wifi.security = live.security;
        }
    }

    for (ssid, ap) in best_by_ssid {
        if saved_ssids.contains(&ssid) {
            continue;
        }
        connections.push(Connection {
            id: ssid.clone(),
            uuid: format!("visible:{ssid}"),
            kind: ConnectionKind::Wifi(ap),
            status: ConnectionStatus::Inactive,
            ip4: None,
            device: None,
            saved: false,
        });
    }
}

/// Returns Wi-Fi signal strength for sorting, or 0 for non-Wi-Fi entries.
pub fn wifi_strength(conn: &Connection) -> u8 {
    match &conn.kind {
        ConnectionKind::Wifi(w) => w.strength,
        _ => 0,
    }
}

// ── NmState ───────────────────────────────────────────────────────────────────

/// Overall NetworkManager connectivity state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NmState {
    Unknown,
    Asleep,
    Disconnected,
    Disconnecting,
    Connecting,
    ConnectedLocal,
    ConnectedSite,
    ConnectedGlobal,
}

impl NmState {
    pub fn from_u32(v: u32) -> Self {
        match v {
            10 => Self::Asleep,
            20 => Self::Disconnected,
            30 => Self::Disconnecting,
            40 => Self::Connecting,
            50 => Self::ConnectedLocal,
            60 => Self::ConnectedSite,
            70 => Self::ConnectedGlobal,
            _ => Self::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Asleep => "Asleep",
            Self::Disconnected => "Disconnected",
            Self::Disconnecting => "Disconnecting…",
            Self::Connecting => "Connecting…",
            Self::ConnectedLocal => "Connected (local only)",
            Self::ConnectedSite => "Connected (site)",
            Self::ConnectedGlobal => "Connected",
        }
    }

    pub fn is_connected(self) -> bool {
        matches!(
            self,
            Self::ConnectedLocal | Self::ConnectedSite | Self::ConnectedGlobal
        )
    }
}

// ── ConnectivityState ─────────────────────────────────────────────────────────

/// Internet connectivity check result (separate from NM state).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectivityState {
    Unknown,
    None,
    Portal,
    Limited,
    Full,
}

impl ConnectivityState {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::None,
            2 => Self::Portal,
            3 => Self::Limited,
            4 => Self::Full,
            _ => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests;
