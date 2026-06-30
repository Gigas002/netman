// SPDX-License-Identifier: GPL-3.0-only

//! Application state machine and main event loop.
//!
//! `run` is the sole entry point from `main`. It owns the terminal, the NM
//! client, and the UI state for the lifetime of the process.

use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use libnetman::connection::{Connection, ConnectionStatus, NmState};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::time;
#[cfg(feature = "dbus")]
use tracing::debug;
use tracing::{info, instrument, warn};

use crate::{settings::Settings, ui};

/// Run the TUI application, blocking until the user exits.
#[instrument(skip(settings))]
pub async fn run(settings: Settings) -> Result<()> {
    let mut app = App::new(&settings).await;

    // Terminal setup.
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    info!("netman started");

    let tick = Duration::from_millis(settings.tick_rate);
    let mut tick_interval = time::interval(tick);

    let result = loop {
        terminal.draw(|frame| ui::draw(frame, &app))?;

        tokio::select! {
            _ = tick_interval.tick() => {
                if let Err(e) = app.refresh().await {
                    warn!(error = %e, "refresh failed");
                }
            }
            ready = tokio::task::spawn_blocking(|| event::poll(Duration::from_millis(50))) => {
                match ready {
                    Ok(Ok(true)) => {
                        match event::read() {
                            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                                match app.handle_key(key.code, key.modifiers) {
                                    Action::Quit => break Ok(()),
                                    Action::Continue => {}
                                }
                            }
                            Ok(Event::Resize(_, _)) => {}
                            Ok(_) => {}
                            Err(e) => break Err(anyhow::anyhow!("terminal event error: {e}")),
                        }
                    }
                    Ok(Ok(false)) => {}
                    Ok(Err(e)) => warn!(error = %e, "event poll error"),
                    Err(e) => break Err(anyhow::anyhow!("task join error: {e}")),
                }
            }
        }
    };

    // Terminal restore.
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

// ── App ───────────────────────────────────────────────────────────────────────

/// Everything that drives the TUI: NM state, UI state, user focus.
pub struct App {
    /// Flat list of connection items (section headers interleaved).
    pub items: Vec<ListItem>,
    /// Index of the currently focused connection (not section headers).
    pub selected: usize,
    /// Whether the detail panel is visible.
    pub show_detail: bool,
    /// Overall NM daemon state.
    pub nm_state: NmState,
    /// Whether the app is operating without a live NM daemon.
    pub demo_mode: bool,
    /// Help overlay visible.
    pub show_help: bool,
    /// Status message shown in the status bar (clears on next refresh).
    pub status_message: Option<String>,
    #[cfg(feature = "dbus")]
    nm: Option<libnetman::nm::NmClient>,
}

/// A single row in the connection list.
pub enum ListItem {
    Header(String),
    Connection(Connection),
}

impl ListItem {
    pub fn as_connection(&self) -> Option<&Connection> {
        match self {
            Self::Connection(c) => Some(c),
            Self::Header(_) => None,
        }
    }

    pub fn is_connection(&self) -> bool {
        matches!(self, Self::Connection(_))
    }
}

/// Outcome of handling a key event.
enum Action {
    Quit,
    Continue,
}

impl App {
    async fn new(settings: &Settings) -> Self {
        let show_detail = settings.show_detail;

        #[cfg(feature = "dbus")]
        {
            match libnetman::nm::NmClient::connect().await {
                Ok(nm) => {
                    let mut app = Self {
                        items: Vec::new(),
                        selected: 0,
                        show_detail,
                        nm_state: NmState::Unknown,
                        demo_mode: false,
                        show_help: false,
                        status_message: None,
                        nm: Some(nm),
                    };
                    let _ = app.refresh().await;
                    return app;
                }
                Err(e) => {
                    warn!(error = %e, "NetworkManager not available, falling back to demo mode");
                }
            }
        }

        // Demo mode: no dbus feature or NM unavailable.
        let mut app = Self {
            items: Vec::new(),
            selected: 0,
            show_detail,
            nm_state: NmState::ConnectedGlobal,
            demo_mode: true,
            show_help: false,
            status_message: Some("Demo mode — NetworkManager not available".into()),
            #[cfg(feature = "dbus")]
            nm: None,
        };
        app.items = demo_connections();
        app
    }

    #[instrument(skip(self))]
    async fn refresh(&mut self) -> Result<()> {
        if self.demo_mode {
            return Ok(());
        }

        #[cfg(feature = "dbus")]
        if let Some(nm) = &self.nm {
            self.nm_state = nm.state().await?;
            let connections = nm.connections().await?;
            self.items = build_list_items(connections);
            // Keep selection in bounds after refresh.
            let conn_count = self.items.iter().filter(|i| i.is_connection()).count();
            if conn_count > 0 {
                self.selected = self.selected.min(conn_count - 1);
            }
            debug!(state = ?self.nm_state, items = self.items.len(), "refreshed");
        }

        Ok(())
    }

    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Action {
        match code {
            KeyCode::Char('q') | KeyCode::Char('Q') => return Action::Quit,
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                return Action::Quit;
            }
            KeyCode::Char('?') => self.show_help = !self.show_help,
            KeyCode::Esc if self.show_help => self.show_help = false,
            KeyCode::Tab | KeyCode::Char('p') => self.show_detail = !self.show_detail,
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Enter => self.connect_selected(),
            KeyCode::Char('d') | KeyCode::Delete => self.disconnect_selected(),
            KeyCode::Char('r') | KeyCode::F(5) => {
                self.status_message = Some("Refreshing…".into());
            }
            _ => {}
        }
        Action::Continue
    }

    fn connection_indices(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.is_connection())
            .map(|(idx, _)| idx)
            .collect()
    }

    fn move_selection(&mut self, delta: i32) {
        let indices = self.connection_indices();
        if indices.is_empty() {
            return;
        }
        let conn_count = indices.len();
        let new = (self.selected as i32 + delta).rem_euclid(conn_count as i32) as usize;
        self.selected = new;
    }

    fn selected_connection(&self) -> Option<&Connection> {
        let indices = self.connection_indices();
        let item_idx = *indices.get(self.selected)?;
        self.items[item_idx].as_connection()
    }

    fn connect_selected(&mut self) {
        if let Some(conn) = self.selected_connection() {
            if conn.is_active() {
                self.status_message = Some(format!("'{}' is already connected.", conn.label()));
            } else {
                self.status_message = Some(format!("Connecting to '{}'…", conn.label()));
            }
        }
    }

    fn disconnect_selected(&mut self) {
        if let Some(conn) = self.selected_connection() {
            if conn.is_active() {
                self.status_message = Some(format!("Disconnecting '{}'…", conn.label()));
            } else {
                self.status_message = Some(format!("'{}' is not active.", conn.label()));
            }
        }
    }
}

// ── List building helpers ─────────────────────────────────────────────────────

fn build_list_items(connections: Vec<Connection>) -> Vec<ListItem> {
    let mut wifi: Vec<Connection> = Vec::new();
    let mut ethernet: Vec<Connection> = Vec::new();
    let mut vpn: Vec<Connection> = Vec::new();
    let mut other: Vec<Connection> = Vec::new();

    for c in connections {
        match &c.kind {
            libnetman::connection::ConnectionKind::Wifi(_) => wifi.push(c),
            libnetman::connection::ConnectionKind::Ethernet => ethernet.push(c),
            libnetman::connection::ConnectionKind::Vpn(_) => vpn.push(c),
            _ => other.push(c),
        }
    }

    let mut items = Vec::new();

    if !wifi.is_empty() {
        items.push(ListItem::Header("Wi-Fi".into()));
        items.extend(wifi.into_iter().map(ListItem::Connection));
    }
    if !ethernet.is_empty() {
        items.push(ListItem::Header("Ethernet".into()));
        items.extend(ethernet.into_iter().map(ListItem::Connection));
    }
    if !vpn.is_empty() {
        items.push(ListItem::Header("VPN".into()));
        items.extend(vpn.into_iter().map(ListItem::Connection));
    }
    if !other.is_empty() {
        items.push(ListItem::Header("Other".into()));
        items.extend(other.into_iter().map(ListItem::Connection));
    }

    items
}

fn demo_connections() -> Vec<ListItem> {
    use libnetman::connection::{
        ConnectionKind, Ip4Config, VpnInfo, WifiInfo, WifiMode, WifiSecurity,
    };

    let demo: Vec<Connection> = vec![
        Connection {
            id: "Home Network".into(),
            uuid: "11111111-0000-0000-0000-000000000001".into(),
            kind: ConnectionKind::Wifi(WifiInfo {
                ssid: "Home Network".into(),
                strength: 87,
                security: WifiSecurity::Wpa3,
                frequency: Some(5180),
                bssid: Some("aa:bb:cc:dd:ee:ff".into()),
                mode: WifiMode::Infrastructure,
            }),
            status: ConnectionStatus::Active,
            ip4: Some(Ip4Config {
                address: "192.168.1.100/24".into(),
                gateway: Some("192.168.1.1".into()),
                nameservers: vec!["1.1.1.1".into(), "8.8.8.8".into()],
            }),
            device: Some("wlan0".into()),
        },
        Connection {
            id: "Neighbour WiFi".into(),
            uuid: "11111111-0000-0000-0000-000000000002".into(),
            kind: ConnectionKind::Wifi(WifiInfo {
                ssid: "Neighbour WiFi".into(),
                strength: 42,
                security: WifiSecurity::Wpa2,
                frequency: Some(2437),
                bssid: Some("11:22:33:44:55:66".into()),
                mode: WifiMode::Infrastructure,
            }),
            status: ConnectionStatus::Inactive,
            ip4: None,
            device: None,
        },
        Connection {
            id: "CoffeeShop".into(),
            uuid: "11111111-0000-0000-0000-000000000003".into(),
            kind: ConnectionKind::Wifi(WifiInfo {
                ssid: "CoffeeShop".into(),
                strength: 61,
                security: WifiSecurity::None,
                frequency: Some(5240),
                bssid: None,
                mode: WifiMode::Infrastructure,
            }),
            status: ConnectionStatus::Inactive,
            ip4: None,
            device: None,
        },
        Connection {
            id: "Wired Connection 1".into(),
            uuid: "22222222-0000-0000-0000-000000000001".into(),
            kind: ConnectionKind::Ethernet,
            status: ConnectionStatus::Inactive,
            ip4: None,
            device: Some("eth0".into()),
        },
        Connection {
            id: "Work VPN".into(),
            uuid: "33333333-0000-0000-0000-000000000001".into(),
            kind: ConnectionKind::Vpn(VpnInfo {
                service_type: "org.freedesktop.NetworkManager.openvpn".into(),
            }),
            status: ConnectionStatus::Inactive,
            ip4: None,
            device: None,
        },
    ];

    build_list_items(demo)
}

// Expose App state to the ui module without making all fields pub(crate).
impl App {
    pub fn selected_conn(&self) -> Option<&Connection> {
        self.selected_connection()
    }
}

#[cfg(test)]
mod tests;
