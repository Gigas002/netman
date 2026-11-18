// NmClient tests run against `fake::FakeNm`, an in-process NetworkManager
// served over a peer-to-peer socket. Its methods take the argument types of
// NM's introspection data, so a call with the wrong D-Bus signature fails here
// the way it fails against the real daemon.

use std::collections::HashMap;

use zbus::{OwnedVariant, Variant};

use crate::connection::{ConnectionProfile, ConnectionStatus, WifiSecurity};

use super::security_from_ap;
use fake::{FakeNm, IpData, Settings};

// Flag values as NM reports them (NM80211ApSecurityFlags): pairwise and
// group CCMP are 0x8 | 0x80, TKIP 0x4 | 0x40.
const CCMP: u32 = 0x8 | 0x80;
const PSK: u32 = 0x100;
const EAP: u32 = 0x200;
const SAE: u32 = 0x400;
const OWE: u32 = 0x800;
const PRIVACY: u32 = 0x1;

#[test]
fn security_from_ap_open() {
    assert_eq!(security_from_ap(0, 0, 0), WifiSecurity::None);
}

#[test]
fn security_from_ap_wep() {
    assert_eq!(security_from_ap(PRIVACY, 0, 0), WifiSecurity::Wep);
}

#[test]
fn security_from_ap_wpa1_psk() {
    assert_eq!(
        security_from_ap(PRIVACY, PSK | 0x4 | 0x40, 0),
        WifiSecurity::Wpa
    );
}

#[test]
fn security_from_ap_wpa2_psk() {
    assert_eq!(security_from_ap(PRIVACY, 0, PSK | CCMP), WifiSecurity::Wpa2);
}

#[test]
fn security_from_ap_wpa3_sae() {
    assert_eq!(security_from_ap(PRIVACY, 0, SAE | CCMP), WifiSecurity::Wpa3);
}

#[test]
fn security_from_ap_wpa2_wpa3_transition_takes_a_password_profile() {
    assert_eq!(
        security_from_ap(PRIVACY, 0, PSK | SAE | CCMP),
        WifiSecurity::Wpa2
    );
}

#[test]
fn security_from_ap_owe_is_not_wep() {
    assert_eq!(security_from_ap(PRIVACY, 0, OWE | CCMP), WifiSecurity::Owe);
}

#[test]
fn security_from_ap_enterprise() {
    assert_eq!(
        security_from_ap(PRIVACY, 0, EAP | CCMP),
        WifiSecurity::Enterprise
    );
}

#[test]
fn security_from_ap_pairwise_wep_bits_are_not_wpa() {
    // 0x1 / 0x2 are PAIR_WEP40 / PAIR_WEP104, not WPA / RSN.
    assert_eq!(security_from_ap(PRIVACY, 0, 0x2), WifiSecurity::Wep);
}

// ── NmClient against the fake ────────────────────────────────────────────────

fn v<T: Into<Variant<'static>>>(value: T) -> OwnedVariant {
    value.into().try_into().expect("OwnedVariant")
}

fn wifi_settings(id: &str, uuid: &str) -> Settings {
    let mut s: Settings = HashMap::new();
    s.insert(
        "connection".into(),
        HashMap::from([
            ("id".into(), v(id.to_owned())),
            ("uuid".into(), v(uuid.to_owned())),
            ("type".into(), v("802-11-wireless")),
        ]),
    );
    s.insert(
        "802-11-wireless".into(),
        HashMap::from([
            ("ssid".into(), v(id.as_bytes().to_vec())),
            ("mode".into(), v("infrastructure")),
        ]),
    );
    s.insert(
        "802-11-wireless-security".into(),
        HashMap::from([("key-mgmt".into(), v("wpa-psk"))]),
    );
    s
}

#[tokio::test]
async fn activate_sends_object_paths() {
    let (fake, nm) = FakeNm::start().await;
    let path = fake.add_connection(wifi_settings("Home", "u-home")).await;

    nm.activate("u-home").await.expect("activate");

    assert_eq!(
        fake.calls_to("ActivateConnection"),
        [format!("ActivateConnection {path} / /")]
    );
    let conns = nm.connections().await.expect("connections");
    assert_eq!(conns[0].status, ConnectionStatus::Active);
}

#[tokio::test]
async fn deactivate_sends_object_path() {
    let (fake, nm) = FakeNm::start().await;
    fake.add_connection(wifi_settings("Home", "u-home")).await;
    nm.activate("u-home").await.expect("activate");

    nm.deactivate("u-home").await.expect("deactivate");

    assert_eq!(fake.calls_to("DeactivateConnection").len(), 1);
    let conns = nm.connections().await.expect("connections");
    assert_eq!(conns[0].status, ConnectionStatus::Inactive);
}

#[tokio::test]
async fn add_and_activate_wifi_sends_object_paths() {
    let (fake, nm) = FakeNm::start().await;
    let dev = fake.add_wifi_device("wlan0").await;

    nm.add_and_activate_wifi("Cafe", WifiSecurity::Wpa2, Some("password1"), false)
        .await
        .expect("add and activate");

    assert_eq!(
        fake.calls_to("AddAndActivateConnection"),
        [format!("AddAndActivateConnection Cafe {dev} /")]
    );
}

#[tokio::test]
async fn update_connection_profile_calls_update2() {
    let (fake, nm) = FakeNm::start().await;
    let path = fake.add_connection(wifi_settings("Home", "u-home")).await;

    let ConnectionProfile::Wifi(mut wifi) = nm.get_connection_profile("u-home").await.unwrap()
    else {
        panic!("not a Wi-Fi profile");
    };
    assert!(wifi.autoconnect);
    wifi.autoconnect = false;
    nm.update_connection_profile("u-home", &ConnectionProfile::Wifi(wifi))
        .await
        .expect("update");

    assert_eq!(fake.calls_to("Update2"), [format!("Update2 {path} 1")]);
    let ConnectionProfile::Wifi(saved) = nm.get_connection_profile("u-home").await.unwrap() else {
        panic!("not a Wi-Fi profile");
    };
    assert!(!saved.autoconnect);
}

#[tokio::test]
async fn delete_connection_removes_it() {
    let (fake, nm) = FakeNm::start().await;
    fake.add_connection(wifi_settings("Home", "u-home")).await;

    nm.delete_connection("u-home").await.expect("delete");

    assert!(nm.connections().await.unwrap().is_empty());
}

#[tokio::test]
async fn set_networking_enabled_calls_enable() {
    let (fake, nm) = FakeNm::start().await;

    nm.set_networking_enabled(false).await.expect("disable");
    assert!(!nm.networking_enabled().await.unwrap());
    nm.set_networking_enabled(true).await.expect("enable");
    assert!(nm.networking_enabled().await.unwrap());

    assert_eq!(fake.calls_to("Enable"), ["Enable false", "Enable true"]);
}

#[tokio::test]
async fn set_wireless_enabled_writes_the_property() {
    let (fake, nm) = FakeNm::start().await;

    nm.set_wireless_enabled(false).await.expect("disable");

    assert!(!nm.wireless_enabled().await.unwrap());
    assert_eq!(
        fake.calls_to("SetWirelessEnabled"),
        ["SetWirelessEnabled false"]
    );
}

#[tokio::test]
async fn access_points_report_their_security() {
    let (fake, nm) = FakeNm::start().await;
    let dev = fake.add_wifi_device("wlan0").await;
    for (ssid, rsn) in [
        ("Mixed", PSK | SAE | CCMP),
        ("Enhanced-Open", OWE | CCMP),
        ("Sae-Only", SAE | CCMP),
    ] {
        fake.add_ap(&dev, ssid.as_bytes(), 50, 5180, PRIVACY, 0, rsn)
            .await;
    }

    let conns = nm.connections().await.expect("connections");
    let security = |ssid: &str| match &conns.iter().find(|c| c.id == ssid).unwrap().kind {
        crate::connection::ConnectionKind::Wifi(w) => w.security,
        other => panic!("{other:?}"),
    };
    assert_eq!(security("Mixed"), WifiSecurity::Wpa2);
    assert_eq!(security("Enhanced-Open"), WifiSecurity::Owe);
    assert_eq!(security("Sae-Only"), WifiSecurity::Wpa3);
}

#[tokio::test]
async fn active_connection_reports_ipv4_and_ipv6_dns() {
    let (fake, nm) = FakeNm::start().await;
    fake.add_connection(wifi_settings("Home", "u-home")).await;
    fake.set_ip(
        "u-home",
        IpData {
            address: "192.168.1.10",
            prefix: 24,
            gateway: "192.168.1.1",
            dns: vec!["1.1.1.1".parse().unwrap()],
        },
        IpData {
            address: "2001:db8::10",
            prefix: 64,
            gateway: "fe80::1",
            dns: vec!["2001:4860:4860::8888".parse().unwrap()],
        },
    )
    .await;
    nm.activate("u-home").await.expect("activate");

    let conn = nm.connections().await.unwrap().remove(0);
    let ip4 = conn.ip4.expect("ip4");
    assert_eq!(ip4.address, "192.168.1.10/24");
    assert_eq!(ip4.nameservers, ["1.1.1.1"]);
    let ip6 = conn.ip6.expect("ip6");
    assert_eq!(ip6.address, "2001:db8::10/64");
    assert_eq!(ip6.gateway.as_deref(), Some("fe80::1"));
    assert_eq!(ip6.nameservers, ["2001:4860:4860::8888"]);
}

/// An in-process NetworkManager double.
mod fake {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use zbus::{ObjectPath, ObjectServer, OwnedObjectPath, OwnedVariant, fdo, interface};

    use super::super::NmClient;

    pub type Settings = HashMap<String, HashMap<String, OwnedVariant>>;

    const NM_PATH: &str = "/org/freedesktop/NetworkManager";
    const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";

    pub struct State {
        pub nm_state: u32,
        pub networking: bool,
        pub wireless: bool,
        pub devices: Vec<OwnedObjectPath>,
        pub active: Vec<(OwnedObjectPath, String)>,
        pub connections: Vec<(OwnedObjectPath, Settings)>,
        /// IP4Config / IP6Config paths of a connection's activation, by UUID.
        pub ip_configs: HashMap<String, (OwnedObjectPath, OwnedObjectPath)>,
        /// Access points, as (device, access point).
        pub access_points: Vec<(OwnedObjectPath, OwnedObjectPath)>,
        /// Method calls NM received, as `"Member arg arg"`.
        pub calls: Vec<String>,
        next_id: u32,
    }

    impl State {
        fn next_path(&mut self, base: &str) -> OwnedObjectPath {
            self.next_id += 1;
            path(&format!("{base}/{}", self.next_id))
        }

        fn uuid_of(&self, conn: &str) -> Option<String> {
            let (_, settings) = self.connections.iter().find(|(p, _)| p.as_str() == conn)?;
            let value = settings.get("connection")?.get("uuid")?;
            <&str>::try_from(value).ok().map(str::to_owned)
        }
    }

    type Shared = Arc<Mutex<State>>;

    fn path(p: &str) -> OwnedObjectPath {
        OwnedObjectPath::try_from(p.to_owned()).expect("object path")
    }

    pub struct FakeNm {
        pub state: Shared,
        server: zbus::Connection,
    }

    impl FakeNm {
        /// Serve NM on one end of a socket pair and connect `NmClient` to the other.
        pub async fn start() -> (Self, NmClient) {
            let state: Shared = Arc::new(Mutex::new(State {
                nm_state: 70,
                networking: true,
                wireless: true,
                devices: Vec::new(),
                active: Vec::new(),
                connections: Vec::new(),
                access_points: Vec::new(),
                ip_configs: HashMap::new(),
                calls: Vec::new(),
                next_id: 0,
            }));
            let (a, b) = std::os::unix::net::UnixStream::pair().expect("socket pair");
            let server = zbus::connection::Builder::unix_stream(a)
                .server(zbus::Guid::generate())
                .p2p()
                .serve_at(NM_PATH, Nm(state.clone()))
                .serve_at(SETTINGS_PATH, SettingsIface(state.clone()))
                .build();
            let client = zbus::connection::Builder::unix_stream(b).p2p().build();
            let (server, client) = tokio::join!(server, client);
            let fake = Self {
                state,
                server: server.expect("server"),
            };
            let nm = NmClient::with_connection(client.expect("client"))
                .await
                .expect("NmClient");
            (fake, nm)
        }

        pub fn calls_to(&self, member: &str) -> Vec<String> {
            let state = self.state.lock().unwrap();
            state
                .calls
                .iter()
                .filter(|c| c.split(' ').next() == Some(member))
                .cloned()
                .collect()
        }

        pub async fn add_connection(&self, settings: Settings) -> OwnedObjectPath {
            add_connection(&self.state, self.server.object_server(), settings).await
        }

        pub async fn add_wifi_device(&self, iface: &str) -> OwnedObjectPath {
            let p = self
                .state
                .lock()
                .unwrap()
                .next_path(&format!("{NM_PATH}/Devices"));
            let server = self.server.object_server();
            server
                .at(
                    &p,
                    Device {
                        interface: iface.into(),
                        device_type: 2,
                    },
                )
                .await
                .expect("serve Device");
            server
                .at(
                    &p,
                    Wireless {
                        state: self.state.clone(),
                        path: p.clone(),
                    },
                )
                .await
                .expect("serve Wireless");
            self.state.lock().unwrap().devices.push(p.clone());
            p
        }

        /// Give `uuid`'s activations these addresses (address/prefix, gateway, DNS).
        pub async fn set_ip(&self, uuid: &str, v4: IpData, v6: IpData) {
            let (p4, p6) = {
                let mut s = self.state.lock().unwrap();
                (
                    s.next_path(&format!("{NM_PATH}/IP4Config")),
                    s.next_path(&format!("{NM_PATH}/IP6Config")),
                )
            };
            let server = self.server.object_server();
            server
                .at(&p4, Ip4Config(v4))
                .await
                .expect("serve IP4Config");
            server
                .at(&p6, Ip6Config(v6))
                .await
                .expect("serve IP6Config");
            self.state
                .lock()
                .unwrap()
                .ip_configs
                .insert(uuid.to_owned(), (p4, p6));
        }

        #[allow(clippy::too_many_arguments)]
        pub async fn add_ap(
            &self,
            device: &OwnedObjectPath,
            ssid: &[u8],
            strength: u8,
            frequency: u32,
            flags: u32,
            wpa_flags: u32,
            rsn_flags: u32,
        ) -> OwnedObjectPath {
            let p = self
                .state
                .lock()
                .unwrap()
                .next_path(&format!("{NM_PATH}/AccessPoint"));
            let ap = AccessPoint {
                ssid: ssid.to_vec(),
                strength,
                frequency,
                flags,
                wpa_flags,
                rsn_flags,
            };
            self.server
                .object_server()
                .at(&p, ap)
                .await
                .expect("serve AccessPoint");
            self.state
                .lock()
                .unwrap()
                .access_points
                .push((device.clone(), p.clone()));
            p
        }
    }

    async fn add_connection(
        state: &Shared,
        server: &ObjectServer,
        settings: Settings,
    ) -> OwnedObjectPath {
        let p = state.lock().unwrap().next_path(SETTINGS_PATH);
        server
            .at(
                &p,
                SettingsConnection {
                    state: state.clone(),
                    path: p.clone(),
                },
            )
            .await
            .expect("serve Settings.Connection");
        state
            .lock()
            .unwrap()
            .connections
            .push((p.clone(), settings));
        p
    }

    async fn activate(
        state: &Shared,
        server: &ObjectServer,
        conn: &str,
        device: &str,
    ) -> fdo::Result<OwnedObjectPath> {
        let (uuid, p) = {
            let mut s = state.lock().unwrap();
            let uuid = s
                .uuid_of(conn)
                .ok_or_else(|| fdo::Error::UnknownObject(conn.into()))?;
            (uuid, s.next_path(&format!("{NM_PATH}/ActiveConnection")))
        };
        let devices = if device == "/" {
            Vec::new()
        } else {
            vec![path(device)]
        };
        server
            .at(
                &p,
                Active {
                    state: state.clone(),
                    uuid: uuid.clone(),
                    devices,
                },
            )
            .await?;
        state.lock().unwrap().active.push((p.clone(), uuid));
        Ok(p)
    }

    struct Nm(Shared);

    #[interface(name = "org.freedesktop.NetworkManager")]
    impl Nm {
        #[zbus(property)]
        fn state(&self) -> u32 {
            self.0.lock().unwrap().nm_state
        }

        #[zbus(property)]
        fn connectivity(&self) -> u32 {
            4
        }

        #[zbus(property)]
        fn devices(&self) -> Vec<OwnedObjectPath> {
            self.0.lock().unwrap().devices.clone()
        }

        #[zbus(property)]
        fn active_connections(&self) -> Vec<OwnedObjectPath> {
            let s = self.0.lock().unwrap();
            s.active.iter().map(|(p, _)| p.clone()).collect()
        }

        /// Read-only, as in NM: networking is switched with `Enable`.
        #[zbus(property)]
        fn networking_enabled(&self) -> bool {
            self.0.lock().unwrap().networking
        }

        #[zbus(property)]
        fn wireless_enabled(&self) -> bool {
            self.0.lock().unwrap().wireless
        }

        #[zbus(property)]
        fn set_wireless_enabled(&mut self, value: bool) {
            let mut s = self.0.lock().unwrap();
            s.calls.push(format!("SetWirelessEnabled {value}"));
            s.wireless = value;
        }

        fn enable(&self, enable: bool) {
            let mut s = self.0.lock().unwrap();
            s.calls.push(format!("Enable {enable}"));
            s.networking = enable;
        }

        async fn activate_connection(
            &self,
            #[zbus(object_server)] server: &ObjectServer,
            connection: ObjectPath<'_>,
            device: ObjectPath<'_>,
            specific_object: ObjectPath<'_>,
        ) -> fdo::Result<OwnedObjectPath> {
            self.0.lock().unwrap().calls.push(format!(
                "ActivateConnection {connection} {device} {specific_object}"
            ));
            activate(&self.0, server, &connection, &device).await
        }

        fn deactivate_connection(&self, active_connection: ObjectPath<'_>) -> fdo::Result<()> {
            let mut s = self.0.lock().unwrap();
            s.calls
                .push(format!("DeactivateConnection {active_connection}"));
            let before = s.active.len();
            s.active
                .retain(|(p, _)| p.as_str() != active_connection.as_str());
            if s.active.len() == before {
                return Err(fdo::Error::UnknownObject(active_connection.to_string()));
            }
            Ok(())
        }

        async fn add_and_activate_connection(
            &self,
            #[zbus(object_server)] server: &ObjectServer,
            connection: Settings,
            device: ObjectPath<'_>,
            specific_object: ObjectPath<'_>,
        ) -> fdo::Result<(OwnedObjectPath, OwnedObjectPath)> {
            let id = connection
                .get("connection")
                .and_then(|c| c.get("id"))
                .and_then(|v| <&str>::try_from(v).ok())
                .unwrap_or_default()
                .to_owned();
            self.0.lock().unwrap().calls.push(format!(
                "AddAndActivateConnection {id} {device} {specific_object}"
            ));
            let conn = add_connection(&self.0, server, connection).await;
            let active = activate(&self.0, server, &conn, &device).await?;
            Ok((conn, active))
        }
    }

    struct SettingsIface(Shared);

    #[interface(name = "org.freedesktop.NetworkManager.Settings")]
    impl SettingsIface {
        fn list_connections(&self) -> Vec<OwnedObjectPath> {
            let s = self.0.lock().unwrap();
            s.connections.iter().map(|(p, _)| p.clone()).collect()
        }

        fn get_connection_by_uuid(&self, uuid: &str) -> fdo::Result<OwnedObjectPath> {
            let s = self.0.lock().unwrap();
            s.connections
                .iter()
                .map(|(p, _)| p)
                .find(|p| s.uuid_of(p).as_deref() == Some(uuid))
                .cloned()
                .ok_or_else(|| fdo::Error::Failed(format!("no connection {uuid}")))
        }

        async fn add_connection(
            &self,
            #[zbus(object_server)] server: &ObjectServer,
            connection: Settings,
        ) -> OwnedObjectPath {
            self.0.lock().unwrap().calls.push("AddConnection".into());
            add_connection(&self.0, server, connection).await
        }
    }

    struct SettingsConnection {
        state: Shared,
        path: OwnedObjectPath,
    }

    impl SettingsConnection {
        fn settings(&self) -> fdo::Result<Settings> {
            let s = self.state.lock().unwrap();
            s.connections
                .iter()
                .find(|(p, _)| *p == self.path)
                .map(|(_, settings)| settings.clone())
                .ok_or_else(|| fdo::Error::UnknownObject(self.path.to_string()))
        }
    }

    #[interface(name = "org.freedesktop.NetworkManager.Settings.Connection")]
    impl SettingsConnection {
        fn get_settings(&self) -> fdo::Result<Settings> {
            self.settings()
        }

        fn get_secrets(&self, setting_name: &str) -> fdo::Result<Settings> {
            let settings = self.settings()?;
            Ok(settings
                .get(setting_name)
                .map(|section| HashMap::from([(setting_name.to_owned(), section.clone())]))
                .unwrap_or_default())
        }

        fn update2(
            &self,
            settings: Settings,
            flags: u32,
            _args: HashMap<String, OwnedVariant>,
        ) -> fdo::Result<HashMap<String, OwnedVariant>> {
            let mut s = self.state.lock().unwrap();
            s.calls.push(format!("Update2 {} {flags}", self.path));
            let entry = s
                .connections
                .iter_mut()
                .find(|(p, _)| *p == self.path)
                .ok_or_else(|| fdo::Error::UnknownObject(self.path.to_string()))?;
            entry.1 = settings;
            Ok(HashMap::new())
        }

        fn delete(&self) {
            let mut s = self.state.lock().unwrap();
            s.calls.push(format!("Delete {}", self.path));
            s.connections.retain(|(p, _)| *p != self.path);
        }
    }

    struct Active {
        state: Shared,
        uuid: String,
        devices: Vec<OwnedObjectPath>,
    }

    impl Active {
        fn ip_config(&self, v6: bool) -> OwnedObjectPath {
            let s = self.state.lock().unwrap();
            match s.ip_configs.get(&self.uuid) {
                Some((p4, p6)) => if v6 { p6 } else { p4 }.clone(),
                None => path("/"),
            }
        }
    }

    #[interface(name = "org.freedesktop.NetworkManager.Connection.Active")]
    impl Active {
        #[zbus(property)]
        fn uuid(&self) -> String {
            self.uuid.clone()
        }

        #[zbus(property)]
        fn state(&self) -> u32 {
            2
        }

        #[zbus(property)]
        fn devices(&self) -> Vec<OwnedObjectPath> {
            self.devices.clone()
        }

        #[zbus(property)]
        fn ip4_config(&self) -> OwnedObjectPath {
            self.ip_config(false)
        }

        #[zbus(property)]
        fn ip6_config(&self) -> OwnedObjectPath {
            self.ip_config(true)
        }
    }

    struct Device {
        interface: String,
        device_type: u32,
    }

    #[interface(name = "org.freedesktop.NetworkManager.Device")]
    impl Device {
        #[zbus(property)]
        fn interface(&self) -> String {
            self.interface.clone()
        }

        #[zbus(property)]
        fn device_type(&self) -> u32 {
            self.device_type
        }

        #[zbus(property)]
        fn state(&self) -> u32 {
            100
        }
    }

    struct Wireless {
        state: Shared,
        path: OwnedObjectPath,
    }

    #[interface(name = "org.freedesktop.NetworkManager.Device.Wireless")]
    impl Wireless {
        fn get_all_access_points(&self) -> Vec<OwnedObjectPath> {
            let s = self.state.lock().unwrap();
            s.access_points
                .iter()
                .filter(|(dev, _)| *dev == self.path)
                .map(|(_, ap)| ap.clone())
                .collect()
        }

        fn request_scan(&self, _options: HashMap<String, OwnedVariant>) {
            let mut s = self.state.lock().unwrap();
            s.calls.push(format!("RequestScan {}", self.path));
        }
    }

    struct AccessPoint {
        ssid: Vec<u8>,
        strength: u8,
        frequency: u32,
        flags: u32,
        wpa_flags: u32,
        rsn_flags: u32,
    }

    #[interface(name = "org.freedesktop.NetworkManager.AccessPoint")]
    impl AccessPoint {
        #[zbus(property)]
        fn ssid(&self) -> Vec<u8> {
            self.ssid.clone()
        }

        #[zbus(property)]
        fn strength(&self) -> u8 {
            self.strength
        }

        #[zbus(property)]
        fn flags(&self) -> u32 {
            self.flags
        }

        #[zbus(property)]
        fn wpa_flags(&self) -> u32 {
            self.wpa_flags
        }

        #[zbus(property)]
        fn rsn_flags(&self) -> u32 {
            self.rsn_flags
        }

        #[zbus(property)]
        fn frequency(&self) -> u32 {
            self.frequency
        }

        #[zbus(property)]
        fn hw_address(&self) -> String {
            "02:00:00:00:00:01".into()
        }
    }

    /// Address, prefix, gateway and DNS servers of a fake IP config.
    pub struct IpData {
        pub address: &'static str,
        pub prefix: u32,
        pub gateway: &'static str,
        pub dns: Vec<std::net::IpAddr>,
    }

    fn address_data(ip: &IpData) -> Vec<HashMap<String, OwnedVariant>> {
        let mut entry = HashMap::new();
        entry.insert("address".into(), super::v(ip.address));
        entry.insert("prefix".into(), super::v(ip.prefix));
        vec![entry]
    }

    struct Ip4Config(IpData);

    #[interface(name = "org.freedesktop.NetworkManager.IP4Config")]
    impl Ip4Config {
        #[zbus(property)]
        fn address_data(&self) -> Vec<HashMap<String, OwnedVariant>> {
            address_data(&self.0)
        }

        #[zbus(property)]
        fn gateway(&self) -> String {
            self.0.gateway.into()
        }

        #[zbus(property)]
        fn nameserver_data(&self) -> Vec<HashMap<String, OwnedVariant>> {
            self.0
                .dns
                .iter()
                .map(|a| HashMap::from([("address".to_owned(), super::v(a.to_string()))]))
                .collect()
        }
    }

    /// As in NM: IPv6 DNS servers are `Nameservers` (`aay`); there is no
    /// `NameserverData` on this interface.
    struct Ip6Config(IpData);

    #[interface(name = "org.freedesktop.NetworkManager.IP6Config")]
    impl Ip6Config {
        #[zbus(property)]
        fn address_data(&self) -> Vec<HashMap<String, OwnedVariant>> {
            address_data(&self.0)
        }

        #[zbus(property)]
        fn gateway(&self) -> String {
            self.0.gateway.into()
        }

        #[zbus(property)]
        fn nameservers(&self) -> Vec<Vec<u8>> {
            self.0
                .dns
                .iter()
                .filter_map(|a| match a {
                    std::net::IpAddr::V6(v6) => Some(v6.octets().to_vec()),
                    std::net::IpAddr::V4(_) => None,
                })
                .collect()
        }
    }
}
