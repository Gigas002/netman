// NmClient tests run against `fake::FakeNm`, an in-process NetworkManager
// served over a peer-to-peer socket. Its methods take the argument types of
// NM's introspection data, so a call with the wrong D-Bus signature fails here
// the way it fails against the real daemon.

use std::collections::HashMap;

use zbus::{OwnedVariant, Variant};

use crate::connection::{ConnectionProfile, ConnectionStatus, WifiSecurity};

use super::security_from_ap;
use fake::{FakeNm, Settings};

#[test]
fn security_from_ap_open() {
    assert_eq!(security_from_ap(0, 0, 0), WifiSecurity::None);
}

#[test]
fn security_from_ap_wpa2_psk() {
    assert_eq!(security_from_ap(0, 0, 0x102), WifiSecurity::Wpa2);
}

#[test]
fn security_from_ap_wpa3_sae() {
    assert_eq!(security_from_ap(0, 0, 0x400), WifiSecurity::Wpa3);
}

#[test]
fn security_from_ap_enterprise() {
    assert_eq!(security_from_ap(0, 0, 0x200), WifiSecurity::Enterprise);
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
                .at(&p, Wireless::default())
                .await
                .expect("serve Wireless");
            self.state.lock().unwrap().devices.push(p.clone());
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
        uuid: String,
        devices: Vec<OwnedObjectPath>,
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
            path("/")
        }

        #[zbus(property)]
        fn ip6_config(&self) -> OwnedObjectPath {
            path("/")
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

    #[derive(Default)]
    struct Wireless {
        access_points: Vec<OwnedObjectPath>,
    }

    #[interface(name = "org.freedesktop.NetworkManager.Device.Wireless")]
    impl Wireless {
        fn get_all_access_points(&self) -> Vec<OwnedObjectPath> {
            self.access_points.clone()
        }

        fn request_scan(&self, _options: HashMap<String, OwnedVariant>) {}
    }
}
