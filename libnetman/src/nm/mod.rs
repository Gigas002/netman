// SPDX-License-Identifier: GPL-3.0-only

//! NetworkManager D-Bus client.
//!
//! This module is only compiled when the `dbus` Cargo feature is enabled.
//! It wraps the `zbus` async interface to the `org.freedesktop.NetworkManager`
//! service and presents the results as the domain types from [`crate::connection`].

mod proxies;

use std::collections::HashMap;
use std::time::Duration;

use tracing::{debug, instrument, warn};
use zbus::Connection;
use zbus::zvariant::OwnedValue;

use crate::{
    Result,
    connection::{
        Connection as NmConn, ConnectionKind, ConnectionStatus, ConnectivityState, Ip4Config,
        NmState, VpnInfo, WifiInfo, WifiMode, WifiSecurity, merge_wifi_scan_data, wifi_strength,
    },
    error::Error,
};
use proxies::{
    AccessPointProxy, ActiveConnectionProxy, DeviceProxy, DeviceWirelessProxy, Ip4ConfigProxy,
    NetworkManagerProxy, SettingsConnectionProxy, SettingsProxy,
};

/// NM device type constant for Wi-Fi adapters.
const DEVICE_TYPE_WIFI: u32 = 2;

/// High-level client for the NetworkManager D-Bus service.
pub struct NmClient {
    conn: Connection,
}

impl NmClient {
    /// Connect to the system D-Bus and verify NetworkManager is reachable.
    #[instrument(name = "nm_client_connect")]
    pub async fn connect() -> Result<Self> {
        let conn = Connection::system()
            .await
            .map_err(|e| Error::DBus(format!("system bus: {e}")))?;

        // Ping NM to confirm it's running.
        let nm = NetworkManagerProxy::new(&conn).await?;
        let _ = nm.state().await?;

        debug!("connected to NetworkManager");
        Ok(Self { conn })
    }

    /// Returns the current overall NM connectivity state.
    pub async fn state(&self) -> Result<NmState> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let raw = nm.state().await?;
        Ok(NmState::from_u32(raw))
    }

    /// Returns the internet connectivity check result.
    pub async fn connectivity(&self) -> Result<ConnectivityState> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let raw = nm.connectivity().await?;
        Ok(ConnectivityState::from_u32(raw))
    }

    /// Returns whether NM has networking enabled (all devices).
    pub async fn networking_enabled(&self) -> Result<bool> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        nm.networking_enabled().await.map_err(Error::from)
    }

    /// Returns whether the Wi-Fi radio is enabled.
    pub async fn wireless_enabled(&self) -> Result<bool> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        nm.wireless_enabled().await.map_err(Error::from)
    }

    /// Enable or disable networking for all devices.
    pub async fn set_networking_enabled(&self, enabled: bool) -> Result<()> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        nm.set_networking_enabled(enabled).await?;
        Ok(())
    }

    /// Enable or disable the Wi-Fi radio.
    pub async fn set_wireless_enabled(&self, enabled: bool) -> Result<()> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        nm.set_wireless_enabled(enabled).await?;
        Ok(())
    }

    /// Returns all connections known to NetworkManager (saved profiles + active).
    pub async fn connections(&self) -> Result<Vec<NmConn>> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let settings = SettingsProxy::new(&self.conn).await?;

        let active_paths = nm.active_connections().await?;
        let saved_paths = settings.list_connections().await?;

        let mut active_proxies: Vec<ActiveConnectionProxy> = Vec::new();
        for p in &active_paths {
            if let Ok(proxy) = ActiveConnectionProxy::builder(&self.conn)
                .path(p.as_str())
                .unwrap()
                .build()
                .await
            {
                active_proxies.push(proxy);
            }
        }

        let mut results = Vec::new();

        for path in &saved_paths {
            match self.build_connection(path, &active_proxies).await {
                Ok(conn) => results.push(conn),
                Err(e) => warn!(?path, error = %e, "skipping connection"),
            }
        }

        if let Ok(access_points) = self.access_points().await {
            merge_wifi_scan_data(&mut results, access_points);
        }

        results.sort_by(|a, b| {
            b.is_active()
                .cmp(&a.is_active())
                .then_with(|| kind_order(&a.kind).cmp(&kind_order(&b.kind)))
                .then_with(|| wifi_strength(b).cmp(&wifi_strength(a)))
                .then_with(|| a.label().cmp(b.label()))
        });

        Ok(results)
    }

    /// Request a Wi-Fi scan on the first wireless device and wait briefly for results.
    pub async fn request_wifi_scan(&self) -> Result<()> {
        let path = self.find_wireless_path().await?;
        let wireless = DeviceWirelessProxy::builder(&self.conn)
            .path(path.as_str())
            .map_err(|e| Error::DBus(e.to_string()))?
            .build()
            .await?;
        wireless.request_scan(HashMap::new()).await?;
        // NM completes the scan asynchronously; allow time before reading APs.
        tokio::time::sleep(Duration::from_secs(2)).await;
        Ok(())
    }

    /// Activate (connect) a saved connection profile.
    pub async fn activate(&self, uuid: &str) -> Result<()> {
        let settings = SettingsProxy::new(&self.conn).await?;
        let path = settings.get_connection_by_uuid(uuid).await?;
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        nm.activate_connection(path.as_str(), "/", "/").await?;
        Ok(())
    }

    /// Subscribe to `ActiveConnection.StateChanged` signals.
    ///
    /// Returns a channel that receives a unit value each time any active
    /// connection reports a state transition. The caller should refresh its
    /// connection list when a message arrives.
    pub async fn watch_active_state_changes(
        &self,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<()>> {
        use tokio_stream::StreamExt;
        use zbus::{MatchRule, message::Type};

        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface("org.freedesktop.NetworkManager.Connection.Active")?
            .member("StateChanged")?
            .build();

        let mut stream = zbus::MessageStream::for_match_rule(rule, &self.conn, Some(64))
            .await
            .map_err(|e| Error::DBus(e.to_string()))?;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        tokio::spawn(async move {
            while stream.next().await.is_some() {
                if tx.send(()).is_err() {
                    break;
                }
            }
        });

        Ok(rx)
    }

    /// Deactivate (disconnect) an active connection by UUID.
    pub async fn deactivate(&self, uuid: &str) -> Result<()> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let active_paths = nm.active_connections().await?;

        for p in &active_paths {
            let proxy = ActiveConnectionProxy::builder(&self.conn)
                .path(p.as_str())
                .unwrap()
                .build()
                .await?;
            if proxy.uuid().await? == uuid {
                nm.deactivate_connection(p.as_str()).await?;
                return Ok(());
            }
        }
        Err(Error::ConnectionNotFound(uuid.to_owned()))
    }

    // ── private helpers ───────────────────────────────────────────────────────

    async fn build_connection(
        &self,
        path: &zbus::zvariant::OwnedObjectPath,
        active_proxies: &[ActiveConnectionProxy<'_>],
    ) -> Result<NmConn> {
        let sc = SettingsConnectionProxy::builder(&self.conn)
            .path(path.as_str())
            .unwrap()
            .build()
            .await?;

        let raw = sc.get_settings().await?;

        let id = get_str_field(&raw, "connection", "id").unwrap_or_else(|| "Unknown".into());
        let uuid = get_str_field(&raw, "connection", "uuid").unwrap_or_default();
        let conn_type = get_str_field(&raw, "connection", "type").unwrap_or_default();

        let (status, device_name, ip4) = self.active_info_for_uuid(&uuid, active_proxies).await;

        let kind = self.build_kind(&conn_type, &raw).await;

        Ok(NmConn {
            id,
            uuid,
            kind,
            status,
            ip4,
            device: device_name,
            saved: true,
        })
    }

    async fn active_info_for_uuid(
        &self,
        uuid: &str,
        active_proxies: &[ActiveConnectionProxy<'_>],
    ) -> (ConnectionStatus, Option<String>, Option<Ip4Config>) {
        for proxy in active_proxies {
            let Ok(u) = proxy.uuid().await else { continue };
            if u != uuid {
                continue;
            }
            let state = proxy.state().await.unwrap_or(0);
            let status = match state {
                1 => ConnectionStatus::Activating,
                2 => ConnectionStatus::Active,
                3 => ConnectionStatus::Deactivating,
                _ => ConnectionStatus::Unknown,
            };

            let device_name = self.first_device_name(proxy).await;
            let ip4 = self.ip4_config(proxy).await;

            return (status, device_name, ip4);
        }
        (ConnectionStatus::Inactive, None, None)
    }

    async fn first_device_name(&self, active: &ActiveConnectionProxy<'_>) -> Option<String> {
        let paths = active.devices().await.ok()?;
        let path = paths.first()?;
        let dev = DeviceProxy::builder(&self.conn)
            .path(path.as_str())
            .ok()?
            .build()
            .await
            .ok()?;
        dev.interface().await.ok()
    }

    async fn ip4_config(&self, active: &ActiveConnectionProxy<'_>) -> Option<Ip4Config> {
        let path = active.ip4_config().await.ok()?;
        if path.as_str() == "/" {
            return None;
        }
        let cfg = Ip4ConfigProxy::builder(&self.conn)
            .path(path.as_str())
            .ok()?
            .build()
            .await
            .ok()?;

        let addresses = cfg.address_data().await.ok()?;
        let address = addresses.first().and_then(|a| {
            let addr = get_str_value(a, "address")?;
            let prefix = get_u32_value(a, "prefix").unwrap_or(24);
            Some(format!("{addr}/{prefix}"))
        })?;

        let gateway = cfg.gateway().await.ok().filter(|g| !g.is_empty());
        let nameservers = cfg
            .nameserver_data()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|ns| get_str_value(&ns, "address"))
            .collect();

        Some(Ip4Config {
            address,
            gateway,
            nameservers,
        })
    }

    async fn build_kind(
        &self,
        conn_type: &str,
        raw: &HashMap<String, HashMap<String, OwnedValue>>,
    ) -> ConnectionKind {
        match conn_type {
            "802-11-wireless" => ConnectionKind::Wifi(self.build_wifi_info(raw)),
            "802-3-ethernet" => ConnectionKind::Ethernet,
            "vpn" => {
                let service_type =
                    get_str_field(raw, "vpn", "service-type").unwrap_or_else(|| "unknown".into());
                ConnectionKind::Vpn(VpnInfo { service_type })
            }
            "loopback" => ConnectionKind::Loopback,
            other => ConnectionKind::Other(other.to_owned()),
        }
    }

    fn build_wifi_info(&self, raw: &HashMap<String, HashMap<String, OwnedValue>>) -> WifiInfo {
        let wifi_section = raw.get("802-11-wireless");

        // SSID is D-Bus type `ay` (array of bytes).
        let ssid = wifi_section
            .and_then(|s| s.get("ssid"))
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| Vec::<u8>::try_from(v).ok())
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();

        let mode = wifi_section
            .and_then(|s| get_str_value(s, "mode"))
            .map(|m| match m.as_str() {
                "infrastructure" => WifiMode::Infrastructure,
                "adhoc" => WifiMode::AdHoc,
                "ap" => WifiMode::Ap,
                "mesh" => WifiMode::Mesh,
                _ => WifiMode::Unknown,
            })
            .unwrap_or(WifiMode::Infrastructure);

        let security = self.detect_wifi_security(raw);

        WifiInfo {
            ssid,
            strength: 0,
            security,
            frequency: None,
            bssid: None,
            mode,
        }
    }

    fn detect_wifi_security(
        &self,
        raw: &HashMap<String, HashMap<String, OwnedValue>>,
    ) -> WifiSecurity {
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

    async fn find_wireless_path(&self) -> Result<zbus::zvariant::OwnedObjectPath> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        for path in nm.devices().await? {
            let dev = DeviceProxy::builder(&self.conn)
                .path(path.as_str())
                .map_err(|e| Error::DBus(e.to_string()))?
                .build()
                .await?;
            if dev.device_type().await? == DEVICE_TYPE_WIFI {
                return Ok(path);
            }
        }
        Err(Error::DeviceNotFound("wireless".into()))
    }

    async fn access_points(&self) -> Result<Vec<WifiInfo>> {
        let path = self.find_wireless_path().await?;
        let wireless = DeviceWirelessProxy::builder(&self.conn)
            .path(path.as_str())
            .map_err(|e| Error::DBus(e.to_string()))?
            .build()
            .await?;
        let paths = wireless.get_all_access_points().await?;
        let mut results = Vec::new();

        for path in paths {
            match self.build_access_point(path.as_str()).await {
                Ok(ap) => results.push(ap),
                Err(e) => warn!(?path, error = %e, "skipping access point"),
            }
        }

        Ok(results)
    }

    async fn build_access_point(&self, path: &str) -> Result<WifiInfo> {
        let ap = AccessPointProxy::builder(&self.conn)
            .path(path)
            .map_err(|e| Error::DBus(e.to_string()))?
            .build()
            .await?;

        let ssid_bytes = ap.ssid().await?;
        let ssid = String::from_utf8(ssid_bytes).unwrap_or_default();
        if ssid.is_empty() {
            return Err(Error::AccessPointNotFound(path.to_owned()));
        }

        let flags = ap.flags().await.unwrap_or(0);
        let wpa_flags = ap.wpa_flags().await.unwrap_or(0);
        let rsn_flags = ap.rsn_flags().await.unwrap_or(0);

        Ok(WifiInfo {
            ssid,
            strength: ap.strength().await.unwrap_or(0),
            security: security_from_ap(flags, wpa_flags, rsn_flags),
            frequency: Some(ap.frequency().await.unwrap_or(0)),
            bssid: Some(ap.hw_address().await.unwrap_or_default()),
            mode: WifiMode::Infrastructure,
        })
    }
}

// ── zvariant value extraction helpers ────────────────────────────────────────

/// Extract a `String` from a nested `section → key` in a NM settings map.
fn get_str_field(
    raw: &HashMap<String, HashMap<String, OwnedValue>>,
    section: &str,
    key: &str,
) -> Option<String> {
    get_str_value(raw.get(section)?, key)
}

/// Extract a `String` from an `OwnedValue` map by key using `TryFrom<&OwnedValue>`.
fn get_str_value(map: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    let v = map.get(key)?;
    <&str>::try_from(v).ok().map(str::to_owned)
}

/// Extract a `u32` from an `OwnedValue` map by key using `TryFrom<&OwnedValue>`.
fn get_u32_value(map: &HashMap<String, OwnedValue>, key: &str) -> Option<u32> {
    let v = map.get(key)?;
    u32::try_from(v).ok()
}

fn kind_order(kind: &ConnectionKind) -> u8 {
    match kind {
        ConnectionKind::Wifi(_) => 0,
        ConnectionKind::Ethernet => 1,
        ConnectionKind::Vpn(_) => 2,
        ConnectionKind::Loopback => 3,
        ConnectionKind::Other(_) => 4,
    }
}

/// Derive Wi-Fi security from NM access-point flag bitmasks.
pub(crate) fn security_from_ap(flags: u32, wpa_flags: u32, rsn_flags: u32) -> WifiSecurity {
    const AP_FLAG_PRIVACY: u32 = 0x1;
    const AP_SEC_PAIR_WPA: u32 = 0x1;
    const AP_SEC_PAIR_RSN: u32 = 0x2;
    const AP_SEC_KEY_MGMT_PSK: u32 = 0x100;
    const AP_SEC_KEY_MGMT_802_1X: u32 = 0x200;
    const AP_SEC_KEY_MGMT_SAE: u32 = 0x400;

    let sec = wpa_flags | rsn_flags;
    if sec & AP_SEC_KEY_MGMT_SAE != 0 {
        return WifiSecurity::Wpa3;
    }
    if sec & AP_SEC_KEY_MGMT_802_1X != 0 {
        return WifiSecurity::Enterprise;
    }
    if sec & (AP_SEC_KEY_MGMT_PSK | AP_SEC_PAIR_RSN | AP_SEC_PAIR_WPA) != 0 {
        return WifiSecurity::Wpa2;
    }
    if wpa_flags != 0 {
        return WifiSecurity::Wpa;
    }
    if flags & AP_FLAG_PRIVACY != 0 {
        return WifiSecurity::Wep;
    }
    WifiSecurity::None
}

#[cfg(test)]
mod tests;
