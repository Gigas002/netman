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

use tracing::{debug, instrument, warn};
use zbus::Connection;
use zbus::{ObjectPath, OwnedVariant};

use crate::{
    Result,
    connection::{
        Connection as NmConn, ConnectionKind, ConnectionProfile, ConnectionStatus,
        ConnectivityState, Ip4Config, Ip6Config, NmState, VpnInfo, WifiInfo, WifiMode,
        WifiSecurity, merge_wifi_scan_data, wifi_strength,
    },
    error::Error,
};
use connection_settings::{
    UPDATE2_TO_DISK, apply_profile, detect_wifi_security, parse_profile, profile_to_settings,
};
use proxies::{
    ActiveConnectionProxy, DeviceProxy, DeviceWirelessProxy, NetworkManagerProxy,
    SettingsConnectionProxy, SettingsProxy,
};

/// The "no object" path NM takes for an optional object argument.
const NO_OBJECT: ObjectPath<'static> = ObjectPath::from_static_str_unchecked("/");

/// NM device type constant for Wi-Fi adapters.
const DEVICE_TYPE_WIFI: u32 = 2;

const NM_SERVICE: &str = "org.freedesktop.NetworkManager";

/// A proxy builder that doesn't cache properties.
///
/// A caching proxy, zbus' default, subscribes to `PropertiesChanged` and
/// fetches every property on first use: on a bus that is GetNameOwner,
/// AddMatch and GetAll, and RemoveMatch when it is dropped. NmClient's proxies
/// live for one call, so that was most of the D-Bus traffic of a refresh.
/// Objects with several properties are read with one `GetAll` (`get_all`).
pub(crate) fn uncached<'a, T>(conn: &Connection) -> zbus::proxy::Builder<'a, T>
where
    T: zbus::proxy::Defaults,
{
    zbus::proxy::Builder::new(conn).cache_properties(zbus::proxy::CacheProperties::No)
}

/// All properties of one interface of an NM object, from a single `GetAll`.
struct Props(HashMap<String, OwnedVariant>);

impl Props {
    fn get<T: TryFrom<OwnedVariant>>(&self, name: &str) -> Option<T> {
        T::try_from(self.0.get(name)?.try_clone().ok()?).ok()
    }
}

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
        Self::with_connection(conn).await
    }

    /// Use an existing D-Bus connection instead of the system bus, and verify
    /// NetworkManager answers on it.
    ///
    /// This is for a private bus or a peer-to-peer link to a test double;
    /// [`NmClient::connect`] is the normal entry point.
    pub async fn with_connection(conn: Connection) -> Result<Self> {
        let client = Self { conn };
        // Ping NM to confirm it's running.
        client.state().await?;

        debug!("connected to NetworkManager");
        Ok(client)
    }

    async fn nm(&self) -> Result<NetworkManagerProxy<'static>> {
        Ok(uncached(&self.conn).build().await?)
    }

    async fn settings(&self) -> Result<SettingsProxy<'static>> {
        Ok(uncached(&self.conn).build().await?)
    }

    async fn get_all(&self, path: &ObjectPath<'_>, interface: &'static str) -> Result<Props> {
        let proxy = uncached::<zbus::fdo::PropertiesProxy>(&self.conn)
            .destination(NM_SERVICE)
            .path(path.to_owned())
            .build()
            .await?;
        let interface = zbus::names::InterfaceName::from_static_str_unchecked(interface);
        Ok(Props(proxy.get_all(interface).await?))
    }

    /// Returns the current overall NM connectivity state.
    pub async fn state(&self) -> Result<NmState> {
        let nm = self.nm().await?;
        let raw = nm.state().await?;
        Ok(NmState::from_u32(raw))
    }

    /// Returns the internet connectivity check result.
    pub async fn connectivity(&self) -> Result<ConnectivityState> {
        let nm = self.nm().await?;
        let raw = nm.connectivity().await?;
        Ok(ConnectivityState::from_u32(raw))
    }

    /// Returns whether NM has networking enabled (all devices).
    pub async fn networking_enabled(&self) -> Result<bool> {
        let nm = self.nm().await?;
        nm.networking_enabled().await.map_err(Error::from)
    }

    /// Returns whether the Wi-Fi radio is enabled.
    pub async fn wireless_enabled(&self) -> Result<bool> {
        let nm = self.nm().await?;
        nm.wireless_enabled().await.map_err(Error::from)
    }

    /// Enable or disable networking for all devices.
    pub async fn set_networking_enabled(&self, enabled: bool) -> Result<()> {
        let nm = self.nm().await?;
        nm.enable(enabled).await?;
        Ok(())
    }

    /// Enable or disable the Wi-Fi radio.
    pub async fn set_wireless_enabled(&self, enabled: bool) -> Result<()> {
        let nm = self.nm().await?;
        nm.set_wireless_enabled(enabled).await?;
        Ok(())
    }

    /// Returns all connections known to NetworkManager (saved profiles + active).
    pub async fn connections(&self) -> Result<Vec<NmConn>> {
        let nm = self.nm().await?;
        let settings = self.settings().await?;

        let active_paths = nm.active_connections().await?;
        let saved_paths = settings.list_connections().await?;

        // Read each active connection once, keyed by profile UUID.
        let mut active = HashMap::new();
        for p in &active_paths {
            match self.active_info(p).await {
                Ok((uuid, info)) => {
                    active.insert(uuid, info);
                }
                Err(e) => warn!(path = ?p, error = %e, "skipping active connection"),
            }
        }

        let mut results = Vec::new();

        for path in &saved_paths {
            match self.build_connection(path, &mut active).await {
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

    /// Request a Wi-Fi scan on the first wireless device.
    ///
    /// Returns as soon as NM accepts the request; NM scans in the background,
    /// updates the access-point list, and then advances
    /// [`last_scan`](Self::last_scan).
    pub async fn request_wifi_scan(&self) -> Result<()> {
        let path = self.find_wireless_path().await?;
        let wireless = uncached::<DeviceWirelessProxy>(&self.conn)
            .path(path.as_str())
            .build()
            .await?;
        wireless.request_scan(HashMap::new()).await?;
        Ok(())
    }

    /// When the first wireless device last finished a scan (CLOCK_BOOTTIME
    /// milliseconds; -1 if never). Compare values to see a scan finish.
    pub async fn last_scan(&self) -> Result<i64> {
        let path = self.find_wireless_path().await?;
        let wireless = uncached::<DeviceWirelessProxy>(&self.conn)
            .path(path.as_str())
            .build()
            .await?;
        Ok(wireless.last_scan().await?)
    }

    /// Activate (connect) a saved connection profile.
    pub async fn activate(&self, uuid: &str) -> Result<()> {
        let settings = self.settings().await?;
        let path = settings.get_connection_by_uuid(uuid).await?;
        let nm = self.nm().await?;
        nm.activate_connection(&path, &NO_OBJECT, &NO_OBJECT)
            .await?;
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
        let nm = self.nm().await?;
        nm.add_and_activate_connection(settings, &device, &NO_OBJECT)
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
        use std::num::NonZeroUsize;

        use tokio_stream::StreamExt;
        use zbus::{MatchRule, message::Type};

        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface("org.freedesktop.NetworkManager.Connection.Active")
            .member("StateChanged")
            .build()?;

        let mut stream =
            zbus::MessageStream::for_match_rule(rule, &self.conn, NonZeroUsize::new(64))
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
        let settings = self.settings().await?;
        let path = settings.get_connection_by_uuid(uuid).await?;
        let (raw, _) = self.load_connection_settings(uuid).await?;
        let updated = apply_profile(&raw, profile)?;
        let sc = uncached::<SettingsConnectionProxy>(&self.conn)
            .path(path.as_str())
            .build()
            .await?;
        sc.update2(updated, UPDATE2_TO_DISK, HashMap::new()).await?;
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
        let nm_settings = self.settings().await?;
        let path = nm_settings.add_connection(settings).await?;
        let sc = uncached::<SettingsConnectionProxy>(&self.conn)
            .path(path.as_str())
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
        let settings = self.settings().await?;
        let path = settings.get_connection_by_uuid(uuid).await?;
        let sc = uncached::<SettingsConnectionProxy>(&self.conn)
            .path(path.as_str())
            .build()
            .await?;
        sc.delete().await?;
        Ok(())
    }

    /// Deactivate (disconnect) an active connection by UUID.
    pub async fn deactivate(&self, uuid: &str) -> Result<()> {
        let nm = self.nm().await?;
        let active_paths = nm.active_connections().await?;

        for p in &active_paths {
            let proxy = uncached::<ActiveConnectionProxy>(&self.conn)
                .path(p.as_str())
                .build()
                .await?;
            if proxy.uuid().await? == uuid {
                nm.deactivate_connection(p).await?;
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
        HashMap<String, HashMap<String, OwnedVariant>>,
        Option<HashMap<String, HashMap<String, OwnedVariant>>>,
    )> {
        let settings = self.settings().await?;
        let path = settings.get_connection_by_uuid(uuid).await?;
        let sc = uncached::<SettingsConnectionProxy>(&self.conn)
            .path(path.as_str())
            .build()
            .await?;
        let raw = sc.get_settings().await?;

        let mut secrets: HashMap<String, HashMap<String, OwnedVariant>> = HashMap::new();
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

        let settings = self.settings().await?;
        for path in settings.list_connections().await? {
            let sc = uncached::<SettingsConnectionProxy>(&self.conn)
                .path(path.as_str())
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
        path: &zbus::OwnedObjectPath,
        active: &mut HashMap<String, ActiveInfo>,
    ) -> Result<NmConn> {
        let sc = uncached::<SettingsConnectionProxy>(&self.conn)
            .path(path.as_str())
            .build()
            .await?;

        let raw = sc.get_settings().await?;

        let id = get_str_field(&raw, "connection", "id").unwrap_or_else(|| "Unknown".into());
        let uuid = get_str_field(&raw, "connection", "uuid").unwrap_or_default();
        let conn_type = get_str_field(&raw, "connection", "type").unwrap_or_default();

        let info = active.remove(&uuid).unwrap_or_default();
        let kind = self.build_kind(&conn_type, &raw).await;

        Ok(NmConn {
            id,
            uuid,
            kind,
            status: info.status,
            ip4: info.ip4,
            ip6: info.ip6,
            device: info.device,
            saved: true,
        })
    }

    /// Profile UUID and live details of one active connection.
    async fn active_info(&self, path: &ObjectPath<'_>) -> Result<(String, ActiveInfo)> {
        let props = self
            .get_all(path, "org.freedesktop.NetworkManager.Connection.Active")
            .await?;
        let uuid: String = props
            .get("Uuid")
            .ok_or_else(|| Error::DBus(format!("{path}: no Uuid")))?;
        let status = match props.get::<u32>("State").unwrap_or(0) {
            1 => ConnectionStatus::Activating,
            2 => ConnectionStatus::Active,
            3 => ConnectionStatus::Deactivating,
            _ => ConnectionStatus::Unknown,
        };

        let devices: Vec<zbus::OwnedObjectPath> = props.get("Devices").unwrap_or_default();
        let device = match devices.first() {
            Some(dev) => self.device_interface(dev).await,
            None => None,
        };
        let ip4 = match props.get::<zbus::OwnedObjectPath>("Ip4Config") {
            Some(p) if p.as_str() != "/" => self.ip4_config(&p).await,
            _ => None,
        };
        let ip6 = match props.get::<zbus::OwnedObjectPath>("Ip6Config") {
            Some(p) if p.as_str() != "/" => self.ip6_config(&p).await,
            _ => None,
        };

        Ok((
            uuid,
            ActiveInfo {
                status,
                device,
                ip4,
                ip6,
            },
        ))
    }

    async fn device_interface(&self, path: &ObjectPath<'_>) -> Option<String> {
        let dev = uncached::<DeviceProxy>(&self.conn)
            .path(path.to_owned())
            .build()
            .await
            .ok()?;
        dev.interface().await.ok()
    }

    async fn ip4_config(&self, path: &ObjectPath<'_>) -> Option<Ip4Config> {
        let props = self
            .get_all(path, "org.freedesktop.NetworkManager.IP4Config")
            .await
            .ok()?;
        let addresses: Vec<HashMap<String, OwnedVariant>> = props.get("AddressData")?;
        let nameservers: Vec<HashMap<String, OwnedVariant>> =
            props.get("NameserverData").unwrap_or_default();
        Some(Ip4Config {
            address: parse_live_ip_config(&addresses, 24)?,
            gateway: props.get::<String>("Gateway").filter(|g| !g.is_empty()),
            nameservers: nameservers
                .iter()
                .filter_map(|ns| get_str_value(ns, "address"))
                .collect(),
        })
    }

    async fn ip6_config(&self, path: &ObjectPath<'_>) -> Option<Ip6Config> {
        let props = self
            .get_all(path, "org.freedesktop.NetworkManager.IP6Config")
            .await
            .ok()?;
        let addresses: Vec<HashMap<String, OwnedVariant>> = props.get("AddressData")?;
        Some(Ip6Config {
            address: parse_live_ip_config(&addresses, 64)?,
            gateway: props.get::<String>("Gateway").filter(|g| !g.is_empty()),
            nameservers: ipv6_nameservers(props.get("Nameservers").unwrap_or_default()),
        })
    }

    async fn build_kind(
        &self,
        conn_type: &str,
        raw: &HashMap<String, HashMap<String, OwnedVariant>>,
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

    fn build_wifi_info(&self, raw: &HashMap<String, HashMap<String, OwnedVariant>>) -> WifiInfo {
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

        let security = detect_wifi_security(raw);

        WifiInfo {
            ssid,
            strength: 0,
            security,
            frequency: None,
            bssid: None,
            mode,
        }
    }

    async fn find_wireless_path(&self) -> Result<zbus::OwnedObjectPath> {
        let nm = self.nm().await?;
        for path in nm.devices().await? {
            let dev = uncached::<DeviceProxy>(&self.conn)
                .path(path.as_str())
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
        let wireless = uncached::<DeviceWirelessProxy>(&self.conn)
            .path(path.as_str())
            .build()
            .await?;
        let paths = wireless.get_all_access_points().await?;
        let mut results = Vec::new();

        for path in paths {
            match self.build_access_point(&path).await {
                Ok(ap) => results.push(ap),
                Err(e) => warn!(?path, error = %e, "skipping access point"),
            }
        }

        Ok(results)
    }

    async fn build_access_point(&self, path: &ObjectPath<'_>) -> Result<WifiInfo> {
        let ap = self
            .get_all(path, "org.freedesktop.NetworkManager.AccessPoint")
            .await?;

        let ssid_bytes: Vec<u8> = ap.get("Ssid").unwrap_or_default();
        let ssid = String::from_utf8(ssid_bytes).unwrap_or_default();
        if ssid.is_empty() {
            return Err(Error::AccessPointNotFound(path.to_string()));
        }

        let flags = ap.get("Flags").unwrap_or(0);
        let wpa_flags = ap.get("WpaFlags").unwrap_or(0);
        let rsn_flags = ap.get("RsnFlags").unwrap_or(0);

        Ok(WifiInfo {
            ssid,
            strength: ap.get("Strength").unwrap_or(0),
            security: security_from_ap(flags, wpa_flags, rsn_flags),
            frequency: ap.get::<u32>("Frequency").filter(|f| *f != 0),
            bssid: ap.get::<String>("HwAddress").filter(|b| !b.is_empty()),
            mode: WifiMode::Infrastructure,
        })
    }
}

/// Live details of an active connection.
struct ActiveInfo {
    status: ConnectionStatus,
    device: Option<String>,
    ip4: Option<Ip4Config>,
    ip6: Option<Ip6Config>,
}

impl Default for ActiveInfo {
    /// A saved profile that isn't active.
    fn default() -> Self {
        Self {
            status: ConnectionStatus::Inactive,
            device: None,
            ip4: None,
            ip6: None,
        }
    }
}

// ── D-Bus variant extraction helpers ─────────────────────────────────────────

/// Extract a `String` from a nested `section → key` in a NM settings map.
fn get_str_field(
    raw: &HashMap<String, HashMap<String, OwnedVariant>>,
    section: &str,
    key: &str,
) -> Option<String> {
    get_str_value(raw.get(section)?, key)
}

/// Extract a `String` from an `OwnedVariant` map by key using `TryFrom<&OwnedVariant>`.
fn get_str_value(map: &HashMap<String, OwnedVariant>, key: &str) -> Option<String> {
    let v = map.get(key)?;
    <&str>::try_from(v).ok().map(str::to_owned)
}

/// Extract a `u32` from an `OwnedVariant` map by key using `TryFrom<&OwnedVariant>`.
fn get_u32_value(map: &HashMap<String, OwnedVariant>, key: &str) -> Option<u32> {
    let v = map.get(key)?;
    u32::try_from(v).ok()
}

/// Format IP6Config `Nameservers` (`aay`, 16 bytes each) as addresses.
fn ipv6_nameservers(raw: Vec<Vec<u8>>) -> Vec<String> {
    raw.into_iter()
        .filter_map(|bytes| <[u8; 16]>::try_from(bytes).ok())
        .map(|octets| std::net::Ipv6Addr::from(octets).to_string())
        .collect()
}

fn parse_live_ip_config(
    addresses: &[HashMap<String, OwnedVariant>],
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

/// Derive Wi-Fi security from NM access-point flag bitmasks
/// (`NM80211ApFlags` and `NM80211ApSecurityFlags`).
pub(crate) fn security_from_ap(flags: u32, wpa_flags: u32, rsn_flags: u32) -> WifiSecurity {
    const AP_FLAG_PRIVACY: u32 = 0x1;
    const AP_SEC_KEY_MGMT_PSK: u32 = 0x100;
    const AP_SEC_KEY_MGMT_802_1X: u32 = 0x200;
    const AP_SEC_KEY_MGMT_SAE: u32 = 0x400;
    const AP_SEC_KEY_MGMT_OWE: u32 = 0x800;
    const AP_SEC_KEY_MGMT_OWE_TM: u32 = 0x1000;
    const AP_SEC_KEY_MGMT_EAP_SUITE_B_192: u32 = 0x2000;

    let sec = wpa_flags | rsn_flags;
    // A WPA2/WPA3 transition network offers both PSK and SAE; a password
    // profile (wpa-psk) works on it, as it does on WPA2.
    if sec & AP_SEC_KEY_MGMT_PSK != 0 {
        return if rsn_flags & AP_SEC_KEY_MGMT_PSK != 0 {
            WifiSecurity::Wpa2
        } else {
            WifiSecurity::Wpa
        };
    }
    if sec & AP_SEC_KEY_MGMT_SAE != 0 {
        return WifiSecurity::Wpa3;
    }
    if sec & (AP_SEC_KEY_MGMT_802_1X | AP_SEC_KEY_MGMT_EAP_SUITE_B_192) != 0 {
        return WifiSecurity::Enterprise;
    }
    if sec & (AP_SEC_KEY_MGMT_OWE | AP_SEC_KEY_MGMT_OWE_TM) != 0 {
        return WifiSecurity::Owe;
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
