// SPDX-License-Identifier: GPL-3.0-only

//! `zbus` D-Bus proxy definitions for the NetworkManager interfaces used by
//! `NmClient`.  Each proxy maps exactly to one D-Bus interface; only the
//! properties and methods consumed by this crate are declared.

use std::collections::HashMap;

use zbus::{proxy, zvariant::OwnedObjectPath, zvariant::OwnedValue};

// ── org.freedesktop.NetworkManager ───────────────────────────────────────────

#[proxy(
    interface = "org.freedesktop.NetworkManager",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager"
)]
pub trait NetworkManager {
    /// Overall NM daemon state (NMState enum as u32).
    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;

    /// Connectivity check state (NMConnectivityState as u32).
    #[zbus(property)]
    fn connectivity(&self) -> zbus::Result<u32>;

    /// All network devices managed by NM.
    #[zbus(property)]
    fn devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

    /// All currently active connections.
    #[zbus(property)]
    fn active_connections(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

    /// Whether networking (all devices) is enabled.
    #[zbus(property)]
    fn networking_enabled(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn set_networking_enabled(&self, value: bool) -> zbus::Result<()>;

    /// Whether Wi-Fi radio is enabled.
    #[zbus(property)]
    fn wireless_enabled(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn set_wireless_enabled(&self, value: bool) -> zbus::Result<()>;

    /// Activate a previously saved connection on a device.
    fn activate_connection(
        &self,
        connection: &str,
        device: &str,
        specific_object: &str,
    ) -> zbus::Result<OwnedObjectPath>;

    /// Deactivate an active connection.
    fn deactivate_connection(&self, active_connection: &str) -> zbus::Result<()>;

    /// Add a new connection from settings and activate it immediately.
    fn add_and_activate_connection(
        &self,
        connection: HashMap<String, HashMap<String, OwnedValue>>,
        device: &str,
        specific_object: &str,
    ) -> zbus::Result<(OwnedObjectPath, OwnedObjectPath)>;
}

// ── org.freedesktop.NetworkManager.Settings ───────────────────────────────────

#[proxy(
    interface = "org.freedesktop.NetworkManager.Settings",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager/Settings"
)]
pub trait Settings {
    /// Return object paths for all saved connection profiles.
    fn list_connections(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

    /// Return the object path for the connection with the given UUID.
    fn get_connection_by_uuid(&self, uuid: &str) -> zbus::Result<OwnedObjectPath>;

    /// Add a new connection profile from settings.
    fn add_connection(
        &self,
        connection: HashMap<String, HashMap<String, OwnedValue>>,
    ) -> zbus::Result<OwnedObjectPath>;
}

// ── org.freedesktop.NetworkManager.Settings.Connection ───────────────────────

#[proxy(
    interface = "org.freedesktop.NetworkManager.Settings.Connection",
    default_service = "org.freedesktop.NetworkManager"
)]
pub trait SettingsConnection {
    /// Return all settings for this connection profile.
    fn get_settings(&self) -> zbus::Result<HashMap<String, HashMap<String, OwnedValue>>>;

    /// Return secrets for the given settings section (e.g. `802-11-wireless-security`).
    fn get_secrets(
        &self,
        setting_name: &str,
    ) -> zbus::Result<HashMap<String, HashMap<String, OwnedValue>>>;

    /// Update connection settings and persist to disk when `flags` includes `TO_DISK`.
    fn update2(
        &self,
        settings: HashMap<String, HashMap<String, OwnedValue>>,
        flags: u32,
    ) -> zbus::Result<(
        HashMap<String, HashMap<String, OwnedValue>>,
        HashMap<String, OwnedValue>,
        HashMap<String, OwnedValue>,
    )>;

    /// Remove this connection profile from NetworkManager.
    fn delete(&self) -> zbus::Result<()>;
}

// ── org.freedesktop.NetworkManager.Connection.Active ─────────────────────────

#[proxy(
    interface = "org.freedesktop.NetworkManager.Connection.Active",
    default_service = "org.freedesktop.NetworkManager"
)]
pub trait ActiveConnection {
    /// UUID of the underlying connection profile.
    #[zbus(property)]
    fn uuid(&self) -> zbus::Result<String>;

    /// Active-connection state (NMActiveConnectionState as u32).
    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;

    /// Devices participating in this active connection.
    #[zbus(property)]
    fn devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

    /// Object path of the IPv4 config (or "/" if none).
    #[zbus(property)]
    fn ip4_config(&self) -> zbus::Result<OwnedObjectPath>;

    /// Object path of the IPv6 config (or "/" if none).
    #[zbus(property)]
    fn ip6_config(&self) -> zbus::Result<OwnedObjectPath>;
}

// ── org.freedesktop.NetworkManager.Device ────────────────────────────────────

#[proxy(
    interface = "org.freedesktop.NetworkManager.Device",
    default_service = "org.freedesktop.NetworkManager"
)]
pub trait Device {
    /// OS-level interface name (e.g. `eth0`, `wlan0`).
    #[zbus(property)]
    fn interface(&self) -> zbus::Result<String>;

    /// NM device type (NMDeviceType as u32).
    #[zbus(property)]
    fn device_type(&self) -> zbus::Result<u32>;

    /// Current device state (NMDeviceState as u32).
    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;
}

// ── org.freedesktop.NetworkManager.Device.Wireless ───────────────────────────

#[proxy(
    interface = "org.freedesktop.NetworkManager.Device.Wireless",
    default_service = "org.freedesktop.NetworkManager"
)]
pub trait DeviceWireless {
    /// Request a Wi-Fi scan (non-blocking; results come via AccessPoints property).
    fn request_scan(&self, options: HashMap<String, OwnedValue>) -> zbus::Result<()>;

    /// All access points visible to this device.
    fn get_all_access_points(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
}

// ── org.freedesktop.NetworkManager.AccessPoint ───────────────────────────────

#[proxy(
    interface = "org.freedesktop.NetworkManager.AccessPoint",
    default_service = "org.freedesktop.NetworkManager"
)]
pub trait AccessPoint {
    /// SSID as raw bytes.
    #[zbus(property)]
    fn ssid(&self) -> zbus::Result<Vec<u8>>;

    /// Signal strength 0–100.
    #[zbus(property)]
    fn strength(&self) -> zbus::Result<u8>;

    /// AP flags bitmask (NM80211ApFlags).
    #[zbus(property)]
    fn flags(&self) -> zbus::Result<u32>;

    /// WPA flags bitmask.
    #[zbus(property)]
    fn wpa_flags(&self) -> zbus::Result<u32>;

    /// RSN/WPA2 flags bitmask.
    #[zbus(property)]
    fn rsn_flags(&self) -> zbus::Result<u32>;

    /// Frequency in MHz.
    #[zbus(property)]
    fn frequency(&self) -> zbus::Result<u32>;

    /// AP hardware address (BSSID).
    #[zbus(property)]
    fn hw_address(&self) -> zbus::Result<String>;
}

// ── org.freedesktop.NetworkManager.IP4Config ─────────────────────────────────

#[proxy(
    interface = "org.freedesktop.NetworkManager.IP4Config",
    default_service = "org.freedesktop.NetworkManager"
)]
pub trait Ip4Config {
    /// List of address data maps (keys: `address`, `prefix`).
    #[zbus(property)]
    fn address_data(&self) -> zbus::Result<Vec<HashMap<String, OwnedValue>>>;

    /// Default gateway address string.
    #[zbus(property)]
    fn gateway(&self) -> zbus::Result<String>;

    /// List of nameserver data maps (key: `address`).
    #[zbus(property)]
    fn nameserver_data(&self) -> zbus::Result<Vec<HashMap<String, OwnedValue>>>;
}

// ── org.freedesktop.NetworkManager.IP6Config ─────────────────────────────────

#[proxy(
    interface = "org.freedesktop.NetworkManager.IP6Config",
    default_service = "org.freedesktop.NetworkManager"
)]
pub trait Ip6Config {
    /// List of address data maps (keys: `address`, `prefix`).
    #[zbus(property)]
    fn address_data(&self) -> zbus::Result<Vec<HashMap<String, OwnedValue>>>;

    /// Default gateway address string.
    #[zbus(property)]
    fn gateway(&self) -> zbus::Result<String>;

    /// List of nameserver data maps (key: `address`).
    #[zbus(property)]
    fn nameserver_data(&self) -> zbus::Result<Vec<HashMap<String, OwnedValue>>>;
}

// ── org.freedesktop.NetworkManager.Device.Modem ───────────────────────────────

#[cfg(feature = "mobile")]
#[proxy(
    interface = "org.freedesktop.NetworkManager.Device.Modem",
    default_service = "org.freedesktop.NetworkManager"
)]
pub trait DeviceModem {
    /// Supported modem capabilities (NMDeviceModemCapabilities bitmask).
    #[zbus(property)]
    fn modem_capabilities(&self) -> zbus::Result<u32>;

    /// Currently active modem capabilities.
    #[zbus(property)]
    fn current_capabilities(&self) -> zbus::Result<u32>;

    /// ModemManager device ID (object path since NM 1.20).
    #[zbus(property)]
    fn device_id(&self) -> zbus::Result<String>;

    /// MCC+MNC operator code when connected.
    #[zbus(property)]
    fn operator_code(&self) -> zbus::Result<String>;

    /// Active APN when connected.
    #[zbus(property)]
    fn apn(&self) -> zbus::Result<String>;
}

// ── org.freedesktop.ModemManager1.Modem ───────────────────────────────────────

#[cfg(feature = "mobile")]
#[proxy(
    interface = "org.freedesktop.ModemManager1.Modem",
    default_service = "org.freedesktop.ModemManager1"
)]
pub trait MmModem {
    /// Signal quality (0–100) and whether the value is recent.
    #[zbus(property)]
    fn signal_quality(&self) -> zbus::Result<(u32, bool)>;

    /// Unlock type required (MMModemLock enum as u32).
    #[zbus(property)]
    fn unlock_required(&self) -> zbus::Result<u32>;

    /// SIM object path.
    #[zbus(property)]
    fn sim(&self) -> zbus::Result<OwnedObjectPath>;

    /// Current access technologies bitmask.
    #[zbus(property)]
    fn access_technologies(&self) -> zbus::Result<u32>;
}

// ── org.freedesktop.ModemManager1.Modem.Modem3gpp ─────────────────────────────

#[cfg(feature = "mobile")]
#[proxy(
    interface = "org.freedesktop.ModemManager1.Modem.Modem3gpp",
    default_service = "org.freedesktop.ModemManager1"
)]
pub trait MmModem3gpp {
    /// Human-readable operator name.
    #[zbus(property)]
    fn operator_name(&self) -> zbus::Result<String>;
}

// ── org.freedesktop.ModemManager1.Sim ─────────────────────────────────────────

#[cfg(feature = "mobile")]
#[proxy(
    interface = "org.freedesktop.ModemManager1.Sim",
    default_service = "org.freedesktop.ModemManager1"
)]
pub trait MmSim {
    /// Send the SIM PIN to unlock the card.
    fn send_pin(&self, pin: &str) -> zbus::Result<()>;
}
