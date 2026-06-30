// SPDX-License-Identifier: GPL-3.0-only

//! Application state machine and main event loop.
//!
//! `run` is the sole entry point from `main`. It owns the terminal, the NM
//! client, and the UI state for the lifetime of the process.

use std::time::Duration;

use crate::ui::TextInput;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use libnetman::connection::{Connection, ConnectionStatus, NmState, WifiSecurity};
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
            state_changed = app.state_watcher.wait() => {
                if state_changed.is_some() {
                    app.clear_inflight_status();
                    if let Err(e) = app.refresh().await {
                        warn!(error = %e, "refresh after state change failed");
                    }
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
                                    Action::Activate(uuid) => app.on_activate(&uuid).await,
                                    Action::Deactivate(uuid) => app.on_deactivate(&uuid).await,
                                    Action::Scan => app.on_scan().await,
                                    Action::ToggleNetworking => app.on_toggle_networking().await,
                                    Action::ToggleWireless => app.on_toggle_wireless().await,
                                    Action::ConnectUnsaved {
                                        ssid,
                                        security,
                                        password,
                                        hidden,
                                    } => app.on_connect_unsaved(ssid, security, password, hidden).await,
                                }
                            }
                            Ok(Event::Paste(text)) => app.handle_paste(&text),
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
    /// Whether NM networking (all devices) is enabled.
    pub networking_enabled: bool,
    /// Whether the Wi-Fi radio is enabled.
    pub wireless_enabled: bool,
    /// Whether the app is operating without a live NM daemon.
    pub demo_mode: bool,
    /// Help overlay visible.
    pub show_help: bool,
    /// Status message shown in the status bar (clears after NM state changes).
    pub status_message: Option<String>,
    /// Wi-Fi password overlay for connecting to unsaved networks.
    pub password_prompt: Option<PasswordPrompt>,
    /// Hidden-network overlay (SSID + password).
    pub hidden_network_prompt: Option<HiddenNetworkPrompt>,
    #[cfg(feature = "dbus")]
    nm: Option<libnetman::nm::NmClient>,
    state_watcher: StateChangeWaiter,
}

/// State for the Wi-Fi password modal.
pub struct PasswordPrompt {
    pub ssid: String,
    pub security: WifiSecurity,
    pub input: TextInput,
    pub show_password: bool,
    pub error: Option<String>,
}

impl PasswordPrompt {
    pub fn new(ssid: String, security: WifiSecurity) -> Self {
        Self {
            ssid,
            security,
            input: TextInput::new(),
            show_password: false,
            error: None,
        }
    }
}

/// Active field in the hidden-network modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenPromptField {
    Ssid,
    Password,
}

/// State for the hidden Wi-Fi connection modal.
pub struct HiddenNetworkPrompt {
    pub ssid: TextInput,
    pub password: TextInput,
    pub focused: HiddenPromptField,
    pub show_password: bool,
    pub error: Option<String>,
}

impl Default for HiddenNetworkPrompt {
    fn default() -> Self {
        Self::new()
    }
}

impl HiddenNetworkPrompt {
    pub fn new() -> Self {
        Self {
            ssid: TextInput::new(),
            password: TextInput::new(),
            focused: HiddenPromptField::Ssid,
            show_password: false,
            error: None,
        }
    }

    fn focused_input_mut(&mut self) -> &mut TextInput {
        match self.focused {
            HiddenPromptField::Ssid => &mut self.ssid,
            HiddenPromptField::Password => &mut self.password,
        }
    }

    fn next_field(&mut self) {
        self.focused = match self.focused {
            HiddenPromptField::Ssid => HiddenPromptField::Password,
            HiddenPromptField::Password => HiddenPromptField::Ssid,
        };
    }
}

/// Waits for NM active-connection state change signals when D-Bus is enabled.
struct StateChangeWaiter {
    #[cfg(feature = "dbus")]
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<()>>,
}

impl StateChangeWaiter {
    #[cfg(feature = "dbus")]
    fn new(rx: Option<tokio::sync::mpsc::UnboundedReceiver<()>>) -> Self {
        Self { rx }
    }

    #[cfg(not(feature = "dbus"))]
    fn new() -> Self {
        Self {}
    }

    async fn wait(&mut self) -> Option<()> {
        #[cfg(feature = "dbus")]
        {
            match self.rx.as_mut() {
                Some(rx) => rx.recv().await,
                None => std::future::pending().await,
            }
        }
        #[cfg(not(feature = "dbus"))]
        {
            std::future::pending().await
        }
    }
}

/// A single row in the connection list.
pub enum ListItem {
    Header(String),
    Connection(Connection),
    HiddenWifiConnect,
}

impl ListItem {
    pub fn as_connection(&self) -> Option<&Connection> {
        match self {
            Self::Connection(c) => Some(c),
            Self::Header(_) | Self::HiddenWifiConnect => None,
        }
    }

    pub fn is_connection(&self) -> bool {
        matches!(self, Self::Connection(_))
    }

    pub fn is_selectable(&self) -> bool {
        matches!(self, Self::Connection(_) | Self::HiddenWifiConnect)
    }
}

/// Outcome of handling a key event.
enum Action {
    Quit,
    Continue,
    Activate(String),
    Deactivate(String),
    Scan,
    ToggleNetworking,
    ToggleWireless,
    ConnectUnsaved {
        ssid: String,
        security: WifiSecurity,
        password: Option<String>,
        hidden: bool,
    },
}

impl App {
    async fn new(settings: &Settings) -> Self {
        let show_detail = settings.show_detail;

        #[cfg(feature = "dbus")]
        {
            match libnetman::nm::NmClient::connect().await {
                Ok(nm) => {
                    let state_rx = nm.watch_active_state_changes().await.ok();
                    let state_watcher = StateChangeWaiter::new(state_rx);
                    let mut app = Self {
                        items: Vec::new(),
                        selected: 0,
                        show_detail,
                        nm_state: NmState::Unknown,
                        networking_enabled: true,
                        wireless_enabled: true,
                        demo_mode: false,
                        show_help: false,
                        status_message: None,
                        password_prompt: None,
                        hidden_network_prompt: None,
                        nm: Some(nm),
                        state_watcher,
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
            networking_enabled: true,
            wireless_enabled: true,
            demo_mode: true,
            show_help: false,
            status_message: Some("Demo mode — NetworkManager not available".into()),
            password_prompt: None,
            hidden_network_prompt: None,
            #[cfg(feature = "dbus")]
            nm: None,
            state_watcher: {
                #[cfg(feature = "dbus")]
                {
                    StateChangeWaiter::new(None)
                }
                #[cfg(not(feature = "dbus"))]
                {
                    StateChangeWaiter::new()
                }
            },
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
            self.networking_enabled = nm.networking_enabled().await.unwrap_or(true);
            self.wireless_enabled = nm.wireless_enabled().await.unwrap_or(true);
            let connections = nm.connections().await?;
            self.items = build_list_items(connections);
            // Keep selection in bounds after refresh.
            let conn_count = self.items.iter().filter(|i| i.is_selectable()).count();
            if conn_count > 0 {
                self.selected = self.selected.min(conn_count - 1);
            }
            debug!(state = ?self.nm_state, items = self.items.len(), "refreshed");
        }

        Ok(())
    }

    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Action {
        if let Some(action) = self.handle_hidden_network_key(code, modifiers) {
            return action;
        }
        if let Some(action) = self.handle_password_key(code, modifiers) {
            return action;
        }

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
            KeyCode::Enter => return self.connect_selected(),
            KeyCode::Char('d') | KeyCode::Delete => return self.disconnect_selected(),
            KeyCode::Char('r') | KeyCode::F(5) => {
                if !self.wireless_enabled {
                    self.status_message = Some("Wi-Fi is disabled.".into());
                    return Action::Continue;
                }
                self.status_message = Some("Scanning…".into());
                return Action::Scan;
            }
            KeyCode::Char('n') => return Action::ToggleNetworking,
            KeyCode::Char('w') => return Action::ToggleWireless,
            _ => {}
        }
        Action::Continue
    }

    fn handle_hidden_network_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> Option<Action> {
        let prompt = self.hidden_network_prompt.as_mut()?;

        match code {
            KeyCode::Esc => {
                self.hidden_network_prompt = None;
                Some(Action::Continue)
            }
            KeyCode::Tab | KeyCode::BackTab => {
                prompt.next_field();
                Some(Action::Continue)
            }
            KeyCode::Enter => {
                let ssid = prompt.ssid.text().trim().to_owned();
                if ssid.is_empty() {
                    prompt.error = Some("SSID is required.".into());
                    prompt.focused = HiddenPromptField::Ssid;
                    return Some(Action::Continue);
                }
                let password = prompt.password.text().to_owned();
                let security = if password.is_empty() {
                    WifiSecurity::None
                } else {
                    WifiSecurity::Wpa2
                };
                Some(Action::ConnectUnsaved {
                    ssid,
                    security,
                    password: if password.is_empty() {
                        None
                    } else {
                        Some(password)
                    },
                    hidden: true,
                })
            }
            KeyCode::Char('h') | KeyCode::Char('H')
                if modifiers.contains(KeyModifiers::CONTROL) =>
            {
                prompt.show_password = !prompt.show_password;
                Some(Action::Continue)
            }
            _ => {
                if prompt.focused_input_mut().handle_key(code, modifiers) {
                    prompt.error = None;
                }
                Some(Action::Continue)
            }
        }
    }

    fn handle_password_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
        let prompt = self.password_prompt.as_mut()?;

        match code {
            KeyCode::Esc => {
                self.password_prompt = None;
                Some(Action::Continue)
            }
            KeyCode::Enter => {
                if prompt.security.is_secured() && prompt.input.is_empty() {
                    prompt.error = Some("Password is required.".into());
                    return Some(Action::Continue);
                }
                let ssid = prompt.ssid.clone();
                let security = prompt.security;
                let password = if prompt.security.is_secured() {
                    Some(prompt.input.text().to_owned())
                } else {
                    None
                };
                Some(Action::ConnectUnsaved {
                    ssid,
                    security,
                    password,
                    hidden: false,
                })
            }
            KeyCode::Char('h') | KeyCode::Char('H')
                if modifiers.contains(KeyModifiers::CONTROL) =>
            {
                prompt.show_password = !prompt.show_password;
                Some(Action::Continue)
            }
            _ => {
                if prompt.input.handle_key(code, modifiers) {
                    prompt.error = None;
                }
                Some(Action::Continue)
            }
        }
    }

    fn handle_paste(&mut self, text: &str) {
        if let Some(prompt) = &mut self.hidden_network_prompt {
            prompt.focused_input_mut().insert_str(text);
            prompt.error = None;
            return;
        }
        if let Some(prompt) = &mut self.password_prompt {
            prompt.input.insert_str(text);
            prompt.error = None;
        }
    }

    fn connection_indices(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.is_selectable())
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

    fn selected_list_item(&self) -> Option<&ListItem> {
        let indices = self.connection_indices();
        let item_idx = *indices.get(self.selected)?;
        self.items.get(item_idx)
    }

    fn selected_connection(&self) -> Option<&Connection> {
        self.selected_list_item()?.as_connection()
    }

    fn connect_selected(&mut self) -> Action {
        if matches!(self.selected_list_item(), Some(ListItem::HiddenWifiConnect)) {
            if !self.demo_mode {
                if !self.networking_enabled {
                    self.status_message = Some("Networking is disabled.".into());
                    return Action::Continue;
                }
                if !self.wireless_enabled {
                    self.status_message = Some("Wi-Fi is disabled.".into());
                    return Action::Continue;
                }
            }
            self.hidden_network_prompt = Some(HiddenNetworkPrompt::new());
            return Action::Continue;
        }

        if self.demo_mode {
            self.status_message = Some("Demo mode — connect not available".into());
            return Action::Continue;
        }

        if !self.networking_enabled {
            self.status_message = Some("Networking is disabled.".into());
            return Action::Continue;
        }

        let Some(conn) = self.selected_connection().cloned() else {
            return Action::Continue;
        };

        if matches!(conn.kind, libnetman::connection::ConnectionKind::Wifi(_))
            && !self.wireless_enabled
        {
            self.status_message = Some("Wi-Fi is disabled.".into());
            return Action::Continue;
        }

        if !conn.is_saved() {
            let libnetman::connection::ConnectionKind::Wifi(wifi) = &conn.kind else {
                return Action::Continue;
            };
            if matches!(wifi.security, WifiSecurity::Enterprise | WifiSecurity::Wep) {
                self.status_message = Some(format!(
                    "{} networks cannot be connected from here.",
                    wifi.security.label()
                ));
                return Action::Continue;
            }
            if wifi.security.is_secured() {
                self.password_prompt = Some(PasswordPrompt::new(wifi.ssid.clone(), wifi.security));
                return Action::Continue;
            }
            return Action::ConnectUnsaved {
                ssid: wifi.ssid.clone(),
                security: wifi.security,
                password: None,
                hidden: false,
            };
        }

        if conn.is_active() {
            self.status_message = Some(format!("'{}' is already connected.", conn.label()));
            return Action::Continue;
        }

        self.status_message = Some("Activating…".into());
        Action::Activate(conn.uuid)
    }

    fn disconnect_selected(&mut self) -> Action {
        if self.demo_mode {
            self.status_message = Some("Demo mode — disconnect not available".into());
            return Action::Continue;
        }

        let Some(conn) = self.selected_connection().cloned() else {
            return Action::Continue;
        };

        if !conn.is_saved() {
            self.status_message = Some(format!("'{}' is a visible network only.", conn.label()));
            return Action::Continue;
        }

        if !conn.is_active() {
            self.status_message = Some(format!("'{}' is not active.", conn.label()));
            return Action::Continue;
        }

        self.status_message = Some("Deactivating…".into());
        Action::Deactivate(conn.uuid)
    }

    async fn on_activate(&mut self, uuid: &str) {
        #[cfg(feature = "dbus")]
        if let Err(e) = self.activate(uuid).await {
            self.status_message = Some(format!("Activation failed: {e}"));
        }
        #[cfg(not(feature = "dbus"))]
        let _ = uuid;
    }

    async fn on_deactivate(&mut self, uuid: &str) {
        #[cfg(feature = "dbus")]
        if let Err(e) = self.deactivate(uuid).await {
            self.status_message = Some(format!("Deactivation failed: {e}"));
        }
        #[cfg(not(feature = "dbus"))]
        let _ = uuid;
    }

    async fn on_scan(&mut self) {
        if self.demo_mode {
            self.status_message = Some("Demo mode — scan not available".into());
            return;
        }

        if !self.wireless_enabled {
            self.status_message = Some("Wi-Fi is disabled.".into());
            return;
        }

        #[cfg(feature = "dbus")]
        if let Some(nm) = &self.nm {
            match nm.request_wifi_scan().await {
                Ok(()) => match self.refresh().await {
                    Ok(()) => self.status_message = None,
                    Err(e) => self.status_message = Some(format!("Scan failed: {e}")),
                },
                Err(e) => self.status_message = Some(format!("Scan failed: {e}")),
            }
        }
    }

    async fn on_toggle_networking(&mut self) {
        if self.demo_mode {
            self.status_message = Some("Demo mode — toggle not available".into());
            return;
        }

        #[cfg(feature = "dbus")]
        if let Some(nm) = &self.nm {
            let enabled = !self.networking_enabled;
            match nm.set_networking_enabled(enabled).await {
                Ok(()) => {
                    self.networking_enabled = enabled;
                    self.status_message = Some(if enabled {
                        "Networking enabled.".into()
                    } else {
                        "Networking disabled.".into()
                    });
                    let _ = self.refresh().await;
                }
                Err(e) => self.status_message = Some(format!("Toggle failed: {e}")),
            }
        }
    }

    async fn on_toggle_wireless(&mut self) {
        if self.demo_mode {
            self.status_message = Some("Demo mode — toggle not available".into());
            return;
        }

        #[cfg(feature = "dbus")]
        if let Some(nm) = &self.nm {
            let enabled = !self.wireless_enabled;
            match nm.set_wireless_enabled(enabled).await {
                Ok(()) => {
                    self.wireless_enabled = enabled;
                    self.status_message = Some(if enabled {
                        "Wi-Fi enabled.".into()
                    } else {
                        "Wi-Fi disabled.".into()
                    });
                    let _ = self.refresh().await;
                }
                Err(e) => self.status_message = Some(format!("Toggle failed: {e}")),
            }
        }
    }

    async fn on_connect_unsaved(
        &mut self,
        ssid: String,
        security: WifiSecurity,
        password: Option<String>,
        hidden: bool,
    ) {
        if self.demo_mode {
            self.password_prompt = None;
            self.hidden_network_prompt = None;
            self.status_message = Some("Demo mode — connect not available".into());
            return;
        }

        #[cfg(feature = "dbus")]
        if let Some(nm) = &self.nm {
            match nm
                .add_and_activate_wifi(&ssid, security, password.as_deref(), hidden)
                .await
            {
                Ok(()) => {
                    self.password_prompt = None;
                    self.hidden_network_prompt = None;
                    self.status_message = Some("Activating…".into());
                }
                Err(e) => {
                    if hidden {
                        if let Some(prompt) = &mut self.hidden_network_prompt {
                            prompt.error = Some(e.to_string());
                        }
                    } else if password.is_some() {
                        if let Some(prompt) = &mut self.password_prompt {
                            prompt.error = Some(e.to_string());
                        }
                    } else {
                        self.password_prompt = None;
                        self.status_message = Some(format!("Connection failed: {e}"));
                    }
                }
            }
        }
        #[cfg(not(feature = "dbus"))]
        let _ = (ssid, security, password, hidden);
    }

    #[cfg(feature = "dbus")]
    async fn activate(&self, uuid: &str) -> libnetman::Result<()> {
        let Some(nm) = &self.nm else {
            return Ok(());
        };
        nm.activate(uuid).await
    }

    #[cfg(feature = "dbus")]
    async fn deactivate(&self, uuid: &str) -> libnetman::Result<()> {
        let Some(nm) = &self.nm else {
            return Ok(());
        };
        nm.deactivate(uuid).await
    }

    fn clear_inflight_status(&mut self) {
        if self
            .status_message
            .as_deref()
            .is_some_and(is_inflight_status)
        {
            self.status_message = None;
        }
    }
}

/// Returns `true` for transient connect/disconnect status messages.
fn is_inflight_status(message: &str) -> bool {
    matches!(message, "Activating…" | "Deactivating…")
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

    wifi.sort_by(|a, b| {
        b.is_active()
            .cmp(&a.is_active())
            .then_with(|| {
                libnetman::connection::wifi_strength(b)
                    .cmp(&libnetman::connection::wifi_strength(a))
            })
            .then_with(|| a.label().cmp(b.label()))
    });

    let mut items = Vec::new();

    items.push(ListItem::Header("Wi-Fi".into()));
    items.extend(wifi.into_iter().map(ListItem::Connection));
    items.push(ListItem::HiddenWifiConnect);
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
            saved: true,
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
            saved: true,
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
            saved: true,
        },
        Connection {
            id: "GuestWiFi".into(),
            uuid: "visible:GuestWiFi".into(),
            kind: ConnectionKind::Wifi(WifiInfo {
                ssid: "GuestWiFi".into(),
                strength: 38,
                security: WifiSecurity::Wpa2,
                frequency: Some(2462),
                bssid: Some("99:88:77:66:55:44".into()),
                mode: WifiMode::Infrastructure,
            }),
            status: ConnectionStatus::Inactive,
            ip4: None,
            device: None,
            saved: false,
        },
        Connection {
            id: "Wired Connection 1".into(),
            uuid: "22222222-0000-0000-0000-000000000001".into(),
            kind: ConnectionKind::Ethernet,
            status: ConnectionStatus::Inactive,
            ip4: None,
            device: Some("eth0".into()),
            saved: true,
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
            saved: true,
        },
    ];

    build_list_items(demo)
}

// Expose App state to the ui module without making all fields pub(crate).
impl App {
    pub fn selected_conn(&self) -> Option<&Connection> {
        self.selected_connection()
    }

    pub fn selected_hidden_wifi(&self) -> bool {
        matches!(self.selected_list_item(), Some(ListItem::HiddenWifiConnect))
    }
}

#[cfg(test)]
mod tests;
