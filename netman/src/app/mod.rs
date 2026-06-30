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
use libnetman::connection::{
    Connection, ConnectionKind, ConnectionProfile, ConnectionStatus, EthernetProfile, Ipv4Profile,
    NmState, VpnProfile, WifiProfile, WifiSecurity,
};
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
                                    Action::EditConnection(uuid) => app.on_edit_connection(uuid).await,
                                    Action::SaveConnection => app.on_save_connection().await,
                                    Action::ImportVpn {
                                        plugin_name,
                                        path,
                                        activate,
                                    } => app.on_import_vpn(plugin_name, path, activate).await,
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
    /// Connection profile editor overlay.
    pub connection_editor: Option<ConnectionEditor>,
    /// Add-connection type picker.
    pub add_connection_menu: Option<AddConnectionMenu>,
    /// VPN-specific add sub-menu.
    pub vpn_add_menu: Option<VpnAddMenu>,
    /// VPN file import overlay.
    pub vpn_import_prompt: Option<VpnImportPrompt>,
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

/// Identifies one editable field in the connection editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditorFieldId {
    Ssid,
    Security,
    Password,
    Hidden,
    IpMethod,
    IpAddress,
    Prefix,
    Gateway,
    Dns,
    Mtu,
    ClonedMac,
    VpnServiceType,
    VpnGateway,
    VpnUsername,
    VpnPassword,
    ConnectionName,
    Activate,
}

/// Whether the editor is creating or updating a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    Edit,
    New,
}

/// Connection type offered in the add-connection menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewConnectionKind {
    Wifi,
    Ethernet,
    Vpn,
}

impl NewConnectionKind {
    pub fn all() -> &'static [Self] {
        &[Self::Wifi, Self::Ethernet, Self::Vpn]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Wifi => "Wi-Fi",
            Self::Ethernet => "Ethernet",
            Self::Vpn => "VPN",
        }
    }
}

/// State for the add-connection type picker.
pub struct AddConnectionMenu {
    pub selected: usize,
}

impl Default for AddConnectionMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl AddConnectionMenu {
    pub fn new() -> Self {
        Self { selected: 0 }
    }

    fn move_selection(&mut self, delta: i32) {
        let count = NewConnectionKind::all().len();
        if count == 0 {
            return;
        }
        self.selected = (self.selected as i32 + delta).rem_euclid(count as i32) as usize;
    }

    pub fn selected_kind(&self) -> NewConnectionKind {
        NewConnectionKind::all()[self.selected]
    }
}

/// VPN add sub-menu listing installed plugins plus import.
pub struct VpnAddMenu {
    pub plugins: Vec<libnetman::vpn_plugins::VpnPluginInfo>,
    pub selected: usize,
}

impl VpnAddMenu {
    pub fn item_count(&self) -> usize {
        self.plugins.len() + 1
    }

    pub fn item_label(&self, idx: usize) -> String {
        if let Some(plugin) = self.plugins.get(idx) {
            format!("Configure {}…", plugin.label)
        } else {
            "[ Import from file… ]".into()
        }
    }

    pub fn is_import(&self, idx: usize) -> bool {
        idx >= self.plugins.len()
    }

    fn move_selection(&mut self, delta: i32) {
        let count = self.item_count();
        if count == 0 {
            return;
        }
        self.selected = (self.selected as i32 + delta).rem_euclid(count as i32) as usize;
    }
}

/// State for the VPN file import modal.
pub struct VpnImportPrompt {
    pub plugins: Vec<libnetman::vpn_plugins::VpnPluginInfo>,
    pub selected_plugin: usize,
    pub path: TextInput,
    pub activate_on_save: bool,
    pub error: Option<String>,
}

impl VpnImportPrompt {
    pub fn new(plugins: Vec<libnetman::vpn_plugins::VpnPluginInfo>) -> Self {
        Self {
            plugins,
            selected_plugin: 0,
            path: TextInput::new(),
            activate_on_save: true,
            error: None,
        }
    }

    fn cycle_plugin(&mut self, forward: bool) {
        if self.plugins.is_empty() {
            return;
        }
        let count = self.plugins.len();
        if forward {
            self.selected_plugin = (self.selected_plugin + 1) % count;
        } else {
            self.selected_plugin = (self.selected_plugin + count - 1) % count;
        }
    }
}

/// State for the connection profile editor modal.
pub struct ConnectionEditor {
    pub mode: EditorMode,
    pub uuid: Option<String>,
    pub title: String,
    pub profile: ConnectionProfile,
    pub fields: Vec<EditorFieldId>,
    pub focused: usize,
    pub inputs: std::collections::HashMap<EditorFieldId, TextInput>,
    pub show_secrets: bool,
    pub activate_on_save: bool,
    pub error: Option<String>,
}

impl ConnectionEditor {
    pub fn edit(uuid: String, title: String, profile: ConnectionProfile) -> Self {
        let fields = editor_fields_for(&profile, false);
        let mut inputs = std::collections::HashMap::new();
        populate_inputs(&mut inputs, &profile);
        Self {
            mode: EditorMode::Edit,
            uuid: Some(uuid),
            title,
            profile,
            fields,
            focused: 0,
            inputs,
            show_secrets: false,
            activate_on_save: false,
            error: None,
        }
    }

    pub fn new_add(title: String, profile: ConnectionProfile) -> Self {
        let fields = editor_fields_for(&profile, true);
        let mut inputs = std::collections::HashMap::new();
        populate_inputs(&mut inputs, &profile);
        Self {
            mode: EditorMode::New,
            uuid: None,
            title,
            profile,
            fields,
            focused: 0,
            inputs,
            show_secrets: false,
            activate_on_save: true,
            error: None,
        }
    }

    pub fn is_new(&self) -> bool {
        self.mode == EditorMode::New
    }

    fn focused_field(&self) -> Option<EditorFieldId> {
        self.fields.get(self.focused).copied()
    }

    fn next_field(&mut self) {
        if self.fields.is_empty() {
            return;
        }
        self.sync_focused_text();
        self.focused = (self.focused + 1) % self.fields.len();
    }

    fn prev_field(&mut self) {
        if self.fields.is_empty() {
            return;
        }
        self.sync_focused_text();
        self.focused = (self.focused + self.fields.len() - 1) % self.fields.len();
    }

    fn sync_focused_text(&mut self) {
        let Some(field) = self.focused_field() else {
            return;
        };
        if !field.is_text() {
            return;
        }
        let Some(input) = self.inputs.get(&field) else {
            return;
        };
        let text = input.text().to_owned();
        apply_text_field(&mut self.profile, field, &text);
    }

    fn sync_all_text(&mut self) {
        for field in &self.fields {
            if field.is_text()
                && let Some(input) = self.inputs.get(field)
            {
                apply_text_field(&mut self.profile, *field, input.text());
            }
        }
    }

    fn cycle_choice(&mut self, forward: bool) {
        let Some(field) = self.focused_field() else {
            return;
        };
        match field {
            EditorFieldId::Security => {
                if let ConnectionProfile::Wifi(w) = &mut self.profile {
                    w.security = if forward {
                        w.security.next_editable()
                    } else {
                        w.security.prev_editable()
                    };
                }
            }
            EditorFieldId::IpMethod => {
                if let Some(ipv4) = profile_ipv4_mut(&mut self.profile) {
                    ipv4.method = if forward {
                        ipv4.method.next()
                    } else {
                        ipv4.method.prev()
                    };
                }
            }
            EditorFieldId::Hidden => {
                if let ConnectionProfile::Wifi(w) = &mut self.profile {
                    w.hidden = !w.hidden;
                }
            }
            EditorFieldId::Activate => {
                self.activate_on_save = !self.activate_on_save;
            }
            _ => {}
        }
    }

    fn focused_input_mut(&mut self) -> Option<&mut TextInput> {
        let field = self.focused_field()?;
        self.inputs.get_mut(&field)
    }

    pub fn display_value(&self, field: EditorFieldId) -> String {
        match field {
            EditorFieldId::Security => {
                if let ConnectionProfile::Wifi(w) = &self.profile {
                    w.security.label().to_owned()
                } else {
                    String::new()
                }
            }
            EditorFieldId::Hidden => {
                if let ConnectionProfile::Wifi(w) = &self.profile {
                    if w.hidden { "yes" } else { "no" }.to_owned()
                } else {
                    String::new()
                }
            }
            EditorFieldId::IpMethod => profile_ipv4_ref(&self.profile)
                .map(|ip| ip.method.label().to_owned())
                .unwrap_or_default(),
            EditorFieldId::VpnServiceType => {
                if let ConnectionProfile::Vpn(v) = &self.profile {
                    v.service_type.clone()
                } else {
                    String::new()
                }
            }
            EditorFieldId::Activate => {
                if self.activate_on_save {
                    "yes".into()
                } else {
                    "no".into()
                }
            }
            _ => String::new(),
        }
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
    EditConnection(String),
    SaveConnection,
    ImportVpn {
        plugin_name: String,
        path: String,
        activate: bool,
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
                        connection_editor: None,
                        add_connection_menu: None,
                        vpn_add_menu: None,
                        vpn_import_prompt: None,
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
            connection_editor: None,
            add_connection_menu: None,
            vpn_add_menu: None,
            vpn_import_prompt: None,
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
        if let Some(action) = self.handle_connection_editor_key(code, modifiers) {
            return action;
        }
        if let Some(action) = self.handle_vpn_import_key(code, modifiers) {
            return action;
        }
        if let Some(action) = self.handle_vpn_add_menu_key(code, modifiers) {
            return action;
        }
        if let Some(action) = self.handle_add_connection_menu_key(code, modifiers) {
            return action;
        }
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
            KeyCode::Char('e') => return self.edit_selected(),
            KeyCode::Char('a') => return self.open_add_connection_menu(),
            _ => {}
        }
        Action::Continue
    }

    fn handle_connection_editor_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> Option<Action> {
        let editor = self.connection_editor.as_mut()?;

        match code {
            KeyCode::Esc => {
                self.connection_editor = None;
                Some(Action::Continue)
            }
            KeyCode::Tab | KeyCode::BackTab => {
                if code == KeyCode::Tab {
                    editor.next_field();
                } else {
                    editor.prev_field();
                }
                Some(Action::Continue)
            }
            KeyCode::Enter => Some(Action::SaveConnection),
            KeyCode::Left => {
                editor.cycle_choice(false);
                Some(Action::Continue)
            }
            KeyCode::Right => {
                editor.cycle_choice(true);
                Some(Action::Continue)
            }
            KeyCode::Char(' ') => {
                if matches!(
                    editor.focused_field(),
                    Some(EditorFieldId::Hidden) | Some(EditorFieldId::Activate)
                ) {
                    editor.cycle_choice(true);
                } else if let Some(input) = editor.focused_input_mut() {
                    input.insert_str(" ");
                    editor.error = None;
                }
                Some(Action::Continue)
            }
            KeyCode::Char('h') | KeyCode::Char('H')
                if modifiers.contains(KeyModifiers::CONTROL) =>
            {
                editor.show_secrets = !editor.show_secrets;
                Some(Action::Continue)
            }
            _ => {
                if let Some(field) = editor.focused_field() {
                    if field.is_read_only() || field.is_choice() || field.is_toggle() {
                        return Some(Action::Continue);
                    }
                    if let Some(input) = editor.focused_input_mut()
                        && input.handle_key(code, modifiers)
                    {
                        editor.error = None;
                    }
                }
                Some(Action::Continue)
            }
        }
    }

    fn handle_add_connection_menu_key(
        &mut self,
        code: KeyCode,
        _modifiers: KeyModifiers,
    ) -> Option<Action> {
        let menu = self.add_connection_menu.as_mut()?;

        match code {
            KeyCode::Esc => {
                self.add_connection_menu = None;
                Some(Action::Continue)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                menu.move_selection(-1);
                Some(Action::Continue)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                menu.move_selection(1);
                Some(Action::Continue)
            }
            KeyCode::Enter => {
                let kind = menu.selected_kind();
                self.add_connection_menu = None;
                if kind == NewConnectionKind::Vpn {
                    self.open_vpn_add_menu();
                } else {
                    self.open_new_connection_editor(kind);
                }
                Some(Action::Continue)
            }
            _ => Some(Action::Continue),
        }
    }

    fn handle_vpn_add_menu_key(
        &mut self,
        code: KeyCode,
        _modifiers: KeyModifiers,
    ) -> Option<Action> {
        let menu = self.vpn_add_menu.as_mut()?;

        match code {
            KeyCode::Esc => {
                self.vpn_add_menu = None;
                Some(Action::Continue)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                menu.move_selection(-1);
                Some(Action::Continue)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                menu.move_selection(1);
                Some(Action::Continue)
            }
            KeyCode::Enter => {
                let selected = menu.selected;
                let plugins = menu.plugins.clone();
                if menu.is_import(selected) {
                    self.vpn_add_menu = None;
                    self.vpn_import_prompt = Some(VpnImportPrompt::new(plugins));
                    Some(Action::Continue)
                } else if let Some(plugin) = plugins.get(selected).cloned() {
                    self.vpn_add_menu = None;
                    self.open_vpn_manual_editor(&plugin);
                    Some(Action::Continue)
                } else {
                    Some(Action::Continue)
                }
            }
            _ => Some(Action::Continue),
        }
    }

    fn handle_vpn_import_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
        let prompt = self.vpn_import_prompt.as_mut()?;

        match code {
            KeyCode::Esc => {
                self.vpn_import_prompt = None;
                Some(Action::Continue)
            }
            KeyCode::Left => {
                prompt.cycle_plugin(false);
                Some(Action::Continue)
            }
            KeyCode::Right => {
                prompt.cycle_plugin(true);
                Some(Action::Continue)
            }
            KeyCode::Char(' ') => {
                prompt.activate_on_save = !prompt.activate_on_save;
                Some(Action::Continue)
            }
            KeyCode::Enter => {
                let path = prompt.path.text().trim().to_owned();
                if path.is_empty() {
                    prompt.error = Some("File path is required.".into());
                    return Some(Action::Continue);
                }
                let Some(plugin) = prompt.plugins.get(prompt.selected_plugin) else {
                    prompt.error = Some("No VPN plugin selected.".into());
                    return Some(Action::Continue);
                };
                Some(Action::ImportVpn {
                    plugin_name: plugin.name.clone(),
                    path,
                    activate: prompt.activate_on_save,
                })
            }
            _ => {
                if prompt.path.handle_key(code, modifiers) {
                    prompt.error = None;
                }
                Some(Action::Continue)
            }
        }
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
        if let Some(prompt) = &mut self.vpn_import_prompt {
            prompt.path.insert_str(text);
            prompt.error = None;
            return;
        }
        if let Some(editor) = &mut self.connection_editor {
            if let Some(input) = editor.focused_input_mut() {
                input.insert_str(text);
                editor.error = None;
            }
            return;
        }
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

    fn edit_selected(&mut self) -> Action {
        let Some(conn) = self.selected_connection().cloned() else {
            return Action::Continue;
        };

        if !conn.is_saved() {
            self.status_message = Some("Only saved profiles can be edited.".into());
            return Action::Continue;
        }

        if self.demo_mode {
            let profile = demo_profile_from_connection(&conn);
            if !profile.is_editable() {
                self.status_message = Some("This connection type cannot be edited.".into());
                return Action::Continue;
            }
            self.connection_editor = Some(ConnectionEditor::edit(
                conn.uuid.clone(),
                conn.label().to_owned(),
                profile,
            ));
            return Action::Continue;
        }

        Action::EditConnection(conn.uuid)
    }

    async fn on_edit_connection(&mut self, uuid: String) {
        #[cfg(feature = "dbus")]
        if let Some(nm) = &self.nm {
            match nm.get_connection_profile(&uuid).await {
                Ok(profile) => {
                    if !profile.is_editable() {
                        self.status_message = Some("This connection type cannot be edited.".into());
                        return;
                    }
                    let title = self
                        .selected_connection()
                        .map(|c| c.label().to_owned())
                        .unwrap_or_else(|| uuid.clone());
                    self.connection_editor = Some(ConnectionEditor::edit(uuid, title, profile));
                }
                Err(e) => self.status_message = Some(format!("Load failed: {e}")),
            }
        }
        #[cfg(not(feature = "dbus"))]
        let _ = uuid;
    }

    async fn on_save_connection(&mut self) {
        let Some(editor) = &mut self.connection_editor else {
            return;
        };
        editor.sync_all_text();

        if self.demo_mode {
            self.connection_editor = None;
            self.status_message = Some("Demo mode — save not available".into());
            return;
        }

        #[cfg(feature = "dbus")]
        if let Some(nm) = &self.nm {
            let result = match editor.mode {
                EditorMode::Edit => {
                    let uuid = editor.uuid.clone().expect("edit mode requires uuid");
                    let profile = editor.profile.clone();
                    nm.update_connection_profile(&uuid, &profile)
                        .await
                        .map(|()| uuid)
                }
                EditorMode::New => {
                    let profile = editor.profile.clone();
                    let activate = editor.activate_on_save;
                    nm.add_connection_profile(&profile, activate).await
                }
            };
            match result {
                Ok(_) => {
                    let activating = editor.mode == EditorMode::New && editor.activate_on_save;
                    self.connection_editor = None;
                    self.status_message = Some(if activating {
                        "Connection saved. Activating…".into()
                    } else {
                        "Connection saved.".into()
                    });
                    let _ = self.refresh().await;
                }
                Err(e) => {
                    if let Some(ed) = &mut self.connection_editor {
                        ed.error = Some(e.to_string());
                    }
                }
            }
        }
    }

    fn open_vpn_add_menu(&mut self) {
        let mut plugins = libnetman::vpn_plugins::list_installed_plugins();
        if plugins.is_empty() {
            plugins = demo_vpn_plugins();
        }
        if plugins.is_empty() {
            self.status_message = Some("No VPN plugins installed.".into());
            return;
        }
        self.vpn_add_menu = Some(VpnAddMenu {
            plugins,
            selected: 0,
        });
    }

    fn open_vpn_manual_editor(&mut self, plugin: &libnetman::vpn_plugins::VpnPluginInfo) {
        self.connection_editor = Some(ConnectionEditor::new_add(
            plugin.label.clone(),
            ConnectionProfile::Vpn(VpnProfile {
                name: plugin.label.clone(),
                service_type: plugin.service_type.clone(),
                gateway: String::new(),
                username: String::new(),
                password: String::new(),
                ipv4: Ipv4Profile::default(),
            }),
        ));
    }

    async fn on_import_vpn(&mut self, plugin_name: String, path: String, activate: bool) {
        if self.demo_mode {
            self.vpn_import_prompt = None;
            self.status_message = Some("Demo mode — import not available".into());
            return;
        }

        #[cfg(feature = "dbus")]
        if let Some(nm) = &self.nm {
            match nm.import_vpn_from_file(&plugin_name, &path, activate).await {
                Ok(_) => {
                    self.vpn_import_prompt = None;
                    self.status_message = Some(if activate {
                        "VPN imported. Activating…".into()
                    } else {
                        "VPN imported.".into()
                    });
                    let _ = self.refresh().await;
                }
                Err(e) => {
                    if let Some(prompt) = &mut self.vpn_import_prompt {
                        prompt.error = Some(e.to_string());
                    }
                }
            }
        }
        #[cfg(not(feature = "dbus"))]
        let _ = (plugin_name, path, activate);
    }

    fn open_add_connection_menu(&mut self) -> Action {
        if self.demo_mode {
            self.add_connection_menu = Some(AddConnectionMenu::new());
            return Action::Continue;
        }
        if !self.networking_enabled {
            self.status_message = Some("Networking is disabled.".into());
            return Action::Continue;
        }
        self.add_connection_menu = Some(AddConnectionMenu::new());
        Action::Continue
    }

    fn open_new_connection_editor(&mut self, kind: NewConnectionKind) {
        let profile = blank_profile_for(kind);
        let title = kind.label().to_owned();
        self.connection_editor = Some(ConnectionEditor::new_add(title, profile));
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

// ── Connection editor helpers ─────────────────────────────────────────────────

impl EditorFieldId {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ssid => "SSID",
            Self::Security => "Security",
            Self::Password => "Password",
            Self::Hidden => "Hidden network",
            Self::IpMethod => "IPv4 method",
            Self::IpAddress => "IPv4 address",
            Self::Prefix => "Prefix length",
            Self::Gateway => "Gateway",
            Self::Dns => "DNS",
            Self::Mtu => "MTU",
            Self::ClonedMac => "Cloned MAC",
            Self::VpnServiceType => "VPN service",
            Self::VpnGateway => "Gateway",
            Self::VpnUsername => "Username",
            Self::VpnPassword => "Password",
            Self::ConnectionName => "Name",
            Self::Activate => "Activate after save",
        }
    }

    pub fn is_text(self) -> bool {
        if self == Self::VpnServiceType {
            return false;
        }
        matches!(
            self,
            Self::Ssid
                | Self::Password
                | Self::IpAddress
                | Self::Prefix
                | Self::Gateway
                | Self::Dns
                | Self::Mtu
                | Self::ClonedMac
                | Self::ConnectionName
                | Self::VpnGateway
                | Self::VpnUsername
                | Self::VpnPassword
        )
    }

    pub fn is_secret(self) -> bool {
        matches!(self, Self::Password | Self::VpnPassword)
    }

    pub fn is_choice(self) -> bool {
        matches!(self, Self::Security | Self::IpMethod)
    }

    pub fn is_toggle(self) -> bool {
        matches!(self, Self::Hidden | Self::Activate)
    }

    pub fn is_read_only(self) -> bool {
        self == Self::VpnServiceType
    }
}

pub(crate) fn editor_fields_for(profile: &ConnectionProfile, is_new: bool) -> Vec<EditorFieldId> {
    let mut fields = match profile {
        ConnectionProfile::Wifi(_) => vec![
            EditorFieldId::Ssid,
            EditorFieldId::Security,
            EditorFieldId::Password,
            EditorFieldId::Hidden,
            EditorFieldId::IpMethod,
            EditorFieldId::IpAddress,
            EditorFieldId::Prefix,
            EditorFieldId::Gateway,
            EditorFieldId::Dns,
        ],
        ConnectionProfile::Ethernet(_) => vec![
            EditorFieldId::ConnectionName,
            EditorFieldId::IpMethod,
            EditorFieldId::IpAddress,
            EditorFieldId::Prefix,
            EditorFieldId::Gateway,
            EditorFieldId::Dns,
            EditorFieldId::Mtu,
            EditorFieldId::ClonedMac,
        ],
        ConnectionProfile::Vpn(_) => vec![
            EditorFieldId::ConnectionName,
            EditorFieldId::VpnServiceType,
            EditorFieldId::VpnGateway,
            EditorFieldId::VpnUsername,
            EditorFieldId::VpnPassword,
            EditorFieldId::IpMethod,
            EditorFieldId::IpAddress,
            EditorFieldId::Prefix,
            EditorFieldId::Gateway,
            EditorFieldId::Dns,
        ],
        ConnectionProfile::Unsupported { .. } => vec![],
    };
    if is_new {
        fields.push(EditorFieldId::Activate);
    }
    fields
}

fn populate_inputs(
    inputs: &mut std::collections::HashMap<EditorFieldId, TextInput>,
    profile: &ConnectionProfile,
) {
    let mut set = |field: EditorFieldId, value: &str| {
        let mut input = TextInput::new();
        input.insert_str(value);
        inputs.insert(field, input);
    };

    match profile {
        ConnectionProfile::Wifi(w) => {
            set(EditorFieldId::Ssid, &w.ssid);
            set(EditorFieldId::Password, &w.psk);
            set(EditorFieldId::IpAddress, &w.ipv4.address);
            set(EditorFieldId::Prefix, &w.ipv4.prefix.to_string());
            set(EditorFieldId::Gateway, &w.ipv4.gateway);
            set(EditorFieldId::Dns, &w.ipv4.dns);
        }
        ConnectionProfile::Ethernet(e) => {
            set(EditorFieldId::ConnectionName, &e.name);
            set(EditorFieldId::IpAddress, &e.ipv4.address);
            set(EditorFieldId::Prefix, &e.ipv4.prefix.to_string());
            set(EditorFieldId::Gateway, &e.ipv4.gateway);
            set(EditorFieldId::Dns, &e.ipv4.dns);
            set(EditorFieldId::Mtu, &e.mtu);
            set(EditorFieldId::ClonedMac, &e.cloned_mac);
        }
        ConnectionProfile::Vpn(v) => {
            set(EditorFieldId::ConnectionName, &v.name);
            set(EditorFieldId::VpnGateway, &v.gateway);
            set(EditorFieldId::VpnUsername, &v.username);
            set(EditorFieldId::VpnPassword, &v.password);
            set(EditorFieldId::IpAddress, &v.ipv4.address);
            set(EditorFieldId::Prefix, &v.ipv4.prefix.to_string());
            set(EditorFieldId::Gateway, &v.ipv4.gateway);
            set(EditorFieldId::Dns, &v.ipv4.dns);
        }
        ConnectionProfile::Unsupported { .. } => {}
    }
}

fn apply_text_field(profile: &mut ConnectionProfile, field: EditorFieldId, text: &str) {
    match profile {
        ConnectionProfile::Wifi(w) => match field {
            EditorFieldId::Ssid => w.ssid = text.to_owned(),
            EditorFieldId::Password => w.psk = text.to_owned(),
            EditorFieldId::IpAddress => w.ipv4.address = text.to_owned(),
            EditorFieldId::Prefix => w.ipv4.prefix = text.parse().unwrap_or(24),
            EditorFieldId::Gateway => w.ipv4.gateway = text.to_owned(),
            EditorFieldId::Dns => w.ipv4.dns = text.to_owned(),
            _ => {}
        },
        ConnectionProfile::Ethernet(e) => match field {
            EditorFieldId::ConnectionName => e.name = text.to_owned(),
            EditorFieldId::IpAddress => e.ipv4.address = text.to_owned(),
            EditorFieldId::Prefix => e.ipv4.prefix = text.parse().unwrap_or(24),
            EditorFieldId::Gateway => e.ipv4.gateway = text.to_owned(),
            EditorFieldId::Dns => e.ipv4.dns = text.to_owned(),
            EditorFieldId::Mtu => e.mtu = text.to_owned(),
            EditorFieldId::ClonedMac => e.cloned_mac = text.to_owned(),
            _ => {}
        },
        ConnectionProfile::Vpn(v) => match field {
            EditorFieldId::ConnectionName => v.name = text.to_owned(),
            EditorFieldId::VpnGateway => v.gateway = text.to_owned(),
            EditorFieldId::VpnUsername => v.username = text.to_owned(),
            EditorFieldId::VpnPassword => v.password = text.to_owned(),
            EditorFieldId::IpAddress => v.ipv4.address = text.to_owned(),
            EditorFieldId::Prefix => v.ipv4.prefix = text.parse().unwrap_or(24),
            EditorFieldId::Gateway => v.ipv4.gateway = text.to_owned(),
            EditorFieldId::Dns => v.ipv4.dns = text.to_owned(),
            _ => {}
        },
        ConnectionProfile::Unsupported { .. } => {}
    }
}

fn profile_ipv4_mut(profile: &mut ConnectionProfile) -> Option<&mut Ipv4Profile> {
    match profile {
        ConnectionProfile::Wifi(w) => Some(&mut w.ipv4),
        ConnectionProfile::Ethernet(e) => Some(&mut e.ipv4),
        ConnectionProfile::Vpn(v) => Some(&mut v.ipv4),
        ConnectionProfile::Unsupported { .. } => None,
    }
}

fn profile_ipv4_ref(profile: &ConnectionProfile) -> Option<&Ipv4Profile> {
    match profile {
        ConnectionProfile::Wifi(w) => Some(&w.ipv4),
        ConnectionProfile::Ethernet(e) => Some(&e.ipv4),
        ConnectionProfile::Vpn(v) => Some(&v.ipv4),
        ConnectionProfile::Unsupported { .. } => None,
    }
}

fn demo_profile_from_connection(conn: &Connection) -> ConnectionProfile {
    match &conn.kind {
        ConnectionKind::Wifi(w) => ConnectionProfile::Wifi(WifiProfile {
            ssid: w.ssid.clone(),
            security: w.security,
            psk: String::new(),
            hidden: false,
            ipv4: Ipv4Profile::default(),
        }),
        ConnectionKind::Ethernet => ConnectionProfile::Ethernet(EthernetProfile {
            name: conn.id.clone(),
            ipv4: Ipv4Profile::default(),
            mtu: String::new(),
            cloned_mac: String::new(),
        }),
        ConnectionKind::Vpn(v) => ConnectionProfile::Vpn(VpnProfile {
            name: conn.id.clone(),
            service_type: v.service_type.clone(),
            gateway: String::new(),
            username: String::new(),
            password: String::new(),
            ipv4: Ipv4Profile::default(),
        }),
        ConnectionKind::Loopback => ConnectionProfile::Unsupported {
            id: conn.id.clone(),
            conn_type: "loopback".into(),
        },
        ConnectionKind::Other(t) => ConnectionProfile::Unsupported {
            id: conn.id.clone(),
            conn_type: t.clone(),
        },
    }
}

fn blank_profile_for(kind: NewConnectionKind) -> ConnectionProfile {
    match kind {
        NewConnectionKind::Wifi => ConnectionProfile::Wifi(WifiProfile {
            ssid: String::new(),
            security: WifiSecurity::Wpa2,
            psk: String::new(),
            hidden: false,
            ipv4: Ipv4Profile::default(),
        }),
        NewConnectionKind::Ethernet => ConnectionProfile::Ethernet(EthernetProfile {
            name: "Wired connection".into(),
            ipv4: Ipv4Profile::default(),
            mtu: String::new(),
            cloned_mac: String::new(),
        }),
        NewConnectionKind::Vpn => ConnectionProfile::Vpn(VpnProfile {
            name: "VPN".into(),
            service_type: "org.freedesktop.NetworkManager.openvpn".into(),
            gateway: String::new(),
            username: String::new(),
            password: String::new(),
            ipv4: Ipv4Profile::default(),
        }),
    }
}

fn demo_vpn_plugins() -> Vec<libnetman::vpn_plugins::VpnPluginInfo> {
    vec![libnetman::vpn_plugins::VpnPluginInfo {
        name: "openvpn".into(),
        service_type: "org.freedesktop.NetworkManager.openvpn".into(),
        label: "OpenVPN".into(),
    }]
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
