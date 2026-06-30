// SPDX-License-Identifier: GPL-3.0-only

//! Connection list widget — the primary navigation surface.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use libnetman::connection::{ConnectionKind, ConnectionStatus};

use crate::{
    app::{App, ListItem as AppListItem},
    ui::{BG_SELECTED, FG_ACCENT, FG_ACTIVE, FG_DIM, FG_HEADER, FG_WARN},
};

/// Render the scrollable connection list into `area`.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    // Build the visual rows.
    let connection_indices: Vec<usize> = app
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| i.is_connection())
        .map(|(idx, _)| idx)
        .collect();

    let selected_item_idx = connection_indices.get(app.selected).copied();

    let rows: Vec<ListItem> = app
        .items
        .iter()
        .enumerate()
        .map(|(idx, item)| match item {
            AppListItem::Header(title) => ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {title} "),
                    Style::default().fg(FG_HEADER).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "─".repeat(area.width.saturating_sub(title.len() as u16 + 4) as usize),
                    Style::default().fg(FG_DIM),
                ),
            ])),
            AppListItem::Connection(conn) => {
                let is_selected = Some(idx) == selected_item_idx;
                build_connection_row(conn, is_selected)
            }
        })
        .collect();

    // Convert our flat selected index (connection-only) to the list index.
    let mut list_state = ListState::default();
    list_state.select(selected_item_idx);

    let list = List::new(rows)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" netman — NetworkManager ")
                .border_style(Style::default().fg(FG_ACCENT)),
        )
        .highlight_style(
            Style::default()
                .bg(BG_SELECTED)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn build_connection_row<'a>(
    conn: &libnetman::connection::Connection,
    _selected: bool,
) -> ListItem<'a> {
    let status_color = match conn.status {
        ConnectionStatus::Active => FG_ACTIVE,
        ConnectionStatus::Activating | ConnectionStatus::Deactivating => FG_WARN,
        ConnectionStatus::Inactive => Color::Reset,
        ConnectionStatus::Unknown => FG_DIM,
    };

    let indicator = Span::styled(
        format!(" {} ", conn.status.indicator()),
        Style::default().fg(status_color),
    );

    match &conn.kind {
        ConnectionKind::Wifi(wifi) => {
            let name = Span::styled(
                format!("{:<24}", truncate(&wifi.ssid, 22)),
                Style::default().fg(if conn.is_active() {
                    FG_ACTIVE
                } else {
                    Color::Reset
                }),
            );

            let visible = if conn.saved {
                Span::raw("")
            } else {
                Span::styled(" +", Style::default().fg(FG_WARN))
            };

            let bar = Span::styled(
                wifi.strength_bar(),
                Style::default().fg(strength_color(wifi.strength)),
            );

            let strength_pct = Span::styled(
                format!(" {:>3}%", wifi.strength),
                Style::default().fg(FG_DIM),
            );

            let lock = if wifi.security.is_secured() {
                Span::styled(" 🔒", Style::default())
            } else {
                Span::raw("   ")
            };

            let band = wifi
                .band_label()
                .map(|b| Span::styled(format!(" {b}"), Style::default().fg(FG_DIM)))
                .unwrap_or_else(|| Span::raw(""));

            ListItem::new(Line::from(vec![
                indicator,
                name,
                visible,
                bar,
                strength_pct,
                lock,
                band,
            ]))
        }
        ConnectionKind::Ethernet => {
            let device = conn.device.as_deref().unwrap_or("eth?");
            let name = Span::styled(
                format!("{:<24}", truncate(&conn.id, 22)),
                Style::default().fg(if conn.is_active() {
                    FG_ACTIVE
                } else {
                    Color::Reset
                }),
            );
            let dev_label = Span::styled(format!(" [{device}]"), Style::default().fg(FG_DIM));
            ListItem::new(Line::from(vec![indicator, name, dev_label]))
        }
        ConnectionKind::Vpn(_) => {
            let name = Span::styled(
                format!("{:<24}", truncate(&conn.id, 22)),
                Style::default().fg(if conn.is_active() {
                    FG_ACTIVE
                } else {
                    Color::Reset
                }),
            );
            let vpn_label = Span::styled(" VPN", Style::default().fg(FG_DIM));
            ListItem::new(Line::from(vec![indicator, name, vpn_label]))
        }
        _ => {
            let name = Span::raw(format!(" {}", truncate(&conn.id, 30)));
            ListItem::new(Line::from(name))
        }
    }
}

fn strength_color(strength: u8) -> Color {
    match strength {
        75..=100 => Color::Green,
        50..=74 => Color::Yellow,
        25..=49 => Color::LightRed,
        _ => Color::Red,
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_owned()
    } else {
        format!("{}…", &chars[..max_chars - 1].iter().collect::<String>())
    }
}
