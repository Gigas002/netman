// SPDX-License-Identifier: GPL-3.0-only

//! NetworkManager D-Bus client.
//!
//! This module is only compiled when the `dbus` Cargo feature is enabled.
//! It wraps the `zbus` async interface to the `org.freedesktop.NetworkManager`
//! service and presents the results as the domain types from [`crate::connection`].

mod connection_settings;
#[cfg(feature = "mobile")]
mod modem;
mod proxies;
mod wifi_settings;

use crate::vpn_plugins;

use std::collections::HashMap;
use std::time::Duration;

use tracing::{debug, instrument, warn};
use zbus::Connection;
use zbus::zvariant::OwnedValue;

use crate::{
    Result,
    connection::{
        Connection as NmConn, ConnectionKind, ConnectionProfile, ConnectionStatus,
        ConnectivityState, Ip4Config, Ip6Config, NmState, VpnInfo, WifiInfo, WifiMode,
        WifiSecurity, merge_wifi_scan_data, wifi_strength,
    },
    error::Error,
};
use connection_settings::{UPDATE2_TO_DISK, apply_profile, parse_profile, profile_to_settings};
use proxies::{
    AccessPointProxy, ActiveConnectionProxy, DeviceProxy, DeviceWirelessProxy, Ip4ConfigProxy,
    Ip6ConfigProxy, NetworkManagerProxy, SettingsConnectionProxy, SettingsProxy,
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

        #[cfg(feature = "mobile")]
        {
            let live = modem::fetch_modem_live_data(&self.conn).await;
            crate::connection::merge_modem_live_data(&mut results, &live);
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

    /// Create a Wi-Fi profile from inline settings and activate it.
    pub async fn add_and_activate_wifi(
        &self,
        ssid: &str,
        security: WifiSecurity,
        password: Option<&str>,
        hidden: bool,
    ) -> Result<()> {
        use wifi_settings::wifi_connection_settings;

        let settings = wifi_connection_settings(ssid, security, password, hidden)?;
        let device = self.find_wireless_path().await?;
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        nm.add_and_activate_connection(settings, device.as_str(), "/")
            .await?;
        Ok(())
    }

    #[cfg(feature = "mobile")]
    /// Send a SIM PIN to unlock a locked mobile broadband modem.
    pub async fn send_sim_pin(&self, pin: &str) -> Result<()> {
        modem::send_sim_pin(&self.conn, pin).await
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

    /// Load editable profile settings for a saved connection.
    pub async fn get_connection_profile(&self, uuid: &str) -> Result<ConnectionProfile> {
        let (raw, secrets) = self.load_connection_settings(uuid).await?;
        Ok(parse_profile(&raw, secrets.as_ref()))
    }

    /// Save edited profile settings via `SettingsConnection::Update2`.
    pub async fn update_connection_profile(
        &self,
        uuid: &str,
        profile: &ConnectionProfile,
    ) -> Result<()> {
        let settings = SettingsProxy::new(&self.conn).await?;
        let path = settings.get_connection_by_uuid(uuid).await?;
        let (raw, _) = self.load_connection_settings(uuid).await?;
        let updated = apply_profile(&raw, profile)?;
        let sc = SettingsConnectionProxy::builder(&self.conn)
            .path(path.as_str())
            .map_err(|e| Error::DBus(e.to_string()))?
            .build()
            .await?;
        sc.update2(updated, UPDATE2_TO_DISK).await?;
        Ok(())
    }

    /// Add a new connection profile via `Settings::AddConnection`.
    ///
    /// Returns the UUID of the created profile. When `activate` is `true`, the
    /// connection is activated immediately after creation.
    pub async fn add_connection_profile(
        &self,
        profile: &ConnectionProfile,
        activate: bool,
    ) -> Result<String> {
        let settings = profile_to_settings(profile)?;
        let nm_settings = SettingsProxy::new(&self.conn).await?;
        let path = nm_settings.add_connection(settings).await?;
        let sc = SettingsConnectionProxy::builder(&self.conn)
            .path(path.as_str())
            .map_err(|e| Error::DBus(e.to_string()))?
            .build()
            .await?;
        let raw = sc.get_settings().await?;
        let uuid = get_str_field(&raw, "connection", "uuid")
            .ok_or_else(|| Error::OperationFailed("new connection has no UUID".into()))?;
        if activate {
            self.activate(&uuid).await?;
        }
        Ok(uuid)
    }

    /// Import a VPN profile from an external file using `nmcli` and the NM VPN
    /// editor plugin for `plugin_name` (e.g. `openvpn`).
    pub async fn import_vpn_from_file(
        &self,
        plugin_name: &str,
        path: &str,
        activate: bool,
    ) -> Result<String> {
        if !std::path::Path::new(path).is_file() {
            return Err(Error::OperationFailed(format!("file not found: {path}")));
        }

        let output = tokio::process::Command::new("nmcli")
            .args(["connection", "import", "type", plugin_name, "file", path])
            .output()
            .await
            .map_err(|e| Error::OperationFailed(format!("nmcli not available: {e}")))?;

        if !output.status.success() {
            let msg = String::from_utf8_lossy(&output.stderr);
            let detail = msg.trim();
            if detail.is_empty() {
                return Err(Error::OperationFailed("VPN import failed".into()));
            }
            return Err(Error::OperationFailed(detail.to_owned()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let uuid = if let Some(uuid) = vpn_plugins::parse_import_uuid(&stdout) {
            uuid
        } else {
            self.uuid_for_imported_connection(&stdout).await?
        };

        if activate {
            self.activate(&uuid).await?;
        }
        Ok(uuid)
    }

    /// Delete a saved connection profile via `SettingsConnection::Delete`.
    pub async fn delete_connection(&self, uuid: &str) -> Result<()> {
        let settings = SettingsProxy::new(&self.conn).await?;
        let path = settings.get_connection_by_uuid(uuid).await?;
        let sc = SettingsConnectionProxy::builder(&self.conn)
            .path(path.as_str())
            .map_err(|e| Error::DBus(e.to_string()))?
            .build()
            .await?;
        sc.delete().await?;
        Ok(())
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

    async fn load_connection_settings(
        &self,
        uuid: &str,
    ) -> Result<(
        HashMap<String, HashMap<String, OwnedValue>>,
        Option<HashMap<String, HashMap<String, OwnedValue>>>,
    )> {
        let settings = SettingsProxy::new(&self.conn).await?;
        let path = settings.get_connection_by_uuid(uuid).await?;
        let sc = SettingsConnectionProxy::builder(&self.conn)
            .path(path.as_str())
            .map_err(|e| Error::DBus(e.to_string()))?
            .build()
            .await?;
        let raw = sc.get_settings().await?;

        let mut secrets: HashMap<String, HashMap<String, OwnedValue>> = HashMap::new();
        if raw.contains_key("802-11-wireless-security")
            && let Ok(s) = sc.get_secrets("802-11-wireless-security").await
        {
            secrets.extend(s);
        }
        if raw.contains_key("vpn")
            && let Ok(s) = sc.get_secrets("vpn").await
        {
            secrets.extend(s);
        }
        let secrets = if secrets.is_empty() {
            None
        } else {
            Some(secrets)
        };

        Ok((raw, secrets))
    }

    async fn uuid_for_imported_connection(&self, stdout: &str) -> Result<String> {
        let name = stdout
            .split("Connection '")
            .nth(1)
            .and_then(|rest| rest.split('\'').next())
            .map(str::trim)
            .filter(|n| !n.is_empty());

        let Some(name) = name else {
            return Err(Error::OperationFailed(
                "could not determine imported connection UUID".into(),
            ));
        };

        let settings = SettingsProxy::new(&self.conn).await?;
        for path in settings.list_connections().await? {
            let sc = SettingsConnectionProxy::builder(&self.conn)
                .path(path.as_str())
                .map_err(|e| Error::DBus(e.to_string()))?
                .build()
                .await?;
            let raw = sc.get_settings().await?;
            if get_str_field(&raw, "connection", "id").as_deref() == Some(name)
                && let Some(uuid) = get_str_field(&raw, "connection", "uuid")
            {
                return Ok(uuid);
            }
        }

        Err(Error::OperationFailed(format!(
            "imported connection '{name}' not found"
        )))
    }

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

        let (status, device_name, ip4, ip6) =
            self.active_info_for_uuid(&uuid, active_proxies).await;

        let kind = self.build_kind(&conn_type, &raw).await;

        Ok(NmConn {
            id,
            uuid,
            kind,
            status,
            ip4,
            ip6,
            device: device_name,
            saved: true,
        })
    }

    async fn active_info_for_uuid(
        &self,
        uuid: &str,
        active_proxies: &[ActiveConnectionProxy<'_>],
    ) -> (
        ConnectionStatus,
        Option<String>,
        Option<Ip4Config>,
        Option<Ip6Config>,
    ) {
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
            let ip6 = self.ip6_config(proxy).await;

            return (status, device_name, ip4, ip6);
        }
        (ConnectionStatus::Inactive, None, None, None)
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

        parse_ip4_config(&cfg).await
    }

    async fn ip6_config(&self, active: &ActiveConnectionProxy<'_>) -> Option<Ip6Config> {
        let path = active.ip6_config().await.ok()?;
        if path.as_str() == "/" {
            return None;
        }
        let cfg = Ip6ConfigProxy::builder(&self.conn)
            .path(path.as_str())
            .ok()?
            .build()
            .await
            .ok()?;

        parse_ip6_config(&cfg).await
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
            #[cfg(feature = "mobile")]
            "gsm" => ConnectionKind::Modem(modem::build_modem_info(raw)),
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

async fn parse_ip4_config(cfg: &Ip4ConfigProxy<'_>) -> Option<Ip4Config> {
    let addresses = cfg.address_data().await.ok()?;
    let parsed = parse_live_ip_config(&addresses, 24)?;
    let gateway = cfg.gateway().await.ok().filter(|g| !g.is_empty());
    let nameservers = cfg
        .nameserver_data()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|ns| get_str_value(&ns, "address"))
        .collect();

    Some(Ip4Config {
        address: parsed,
        gateway,
        nameservers,
    })
}

async fn parse_ip6_config(cfg: &Ip6ConfigProxy<'_>) -> Option<Ip6Config> {
    let addresses = cfg.address_data().await.ok()?;
    let parsed = parse_live_ip_config(&addresses, 64)?;
    let gateway = cfg.gateway().await.ok().filter(|g| !g.is_empty());
    let nameservers = cfg
        .nameserver_data()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|ns| get_str_value(&ns, "address"))
        .collect();

    Some(Ip6Config {
        address: parsed,
        gateway,
        nameservers,
    })
}

fn parse_live_ip_config(
    addresses: &[HashMap<String, OwnedValue>],
    default_prefix: u32,
) -> Option<String> {
    addresses.first().and_then(|a| {
        let addr = get_str_value(a, "address")?;
        let prefix = get_u32_value(a, "prefix").unwrap_or(default_prefix);
        Some(format!("{addr}/{prefix}"))
    })
}

fn kind_order(kind: &ConnectionKind) -> u8 {
    match kind {
        ConnectionKind::Wifi(_) => 0,
        ConnectionKind::Ethernet => 1,
        ConnectionKind::Vpn(_) => 2,
        #[cfg(feature = "mobile")]
        ConnectionKind::Modem(_) => 3,
        ConnectionKind::Loopback => {
            if cfg!(feature = "mobile") {
                4
            } else {
                3
            }
        }
        ConnectionKind::Other(_) => {
            if cfg!(feature = "mobile") {
                5
            } else {
                4
            }
        }
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

#[cfg(feature = "dbus")]
pub use crate::vpn_plugins::{VpnPluginInfo, list_installed_plugins};

#[cfg(test)]
mod tests;
