// SPDX-License-Identifier: GPL-3.0-only

//! TUI rendering layer.
//!
//! Each sub-module owns exactly one visual element.  `draw` is the single
//! entry point called from the event loop: it lays out the frame and delegates
//! to each element's render function.

mod add_connection_menu;
mod connection_editor;
mod connection_list;
mod delete_confirm_prompt;
mod detail_panel;
mod hidden_network_prompt;
mod password_prompt;
#[cfg(feature = "mobile")]
mod pin_prompt;
mod status_bar;
mod text_input;
mod vpn_add_menu;
mod vpn_import_prompt;

#[cfg(test)]
mod tests;

pub use text_input::TextInput;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::App;

// ── Colour palette ────────────────────────────────────────────────────────────

pub(crate) const FG_ACTIVE: Color = Color::Green;
pub(crate) const FG_DIM: Color = Color::DarkGray;
pub(crate) const FG_ACCENT: Color = Color::Cyan;
pub(crate) const FG_WARN: Color = Color::Yellow;
pub(crate) const FG_HEADER: Color = Color::Blue;
pub(crate) const BG_SELECTED: Color = Color::DarkGray;

/// Main draw call — invoked once per tick from the event loop.
pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Outer vertical split: body + status + keys.
    let [body_area, status_area, keys_area] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    // Horizontal split for body: list + optional detail panel.
    let list_area;
    let detail_area;
    if app.show_detail {
        let split = Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(body_area);
        list_area = split[0];
        detail_area = Some(split[1]);
    } else {
        list_area = body_area;
        detail_area = None;
    }

    connection_list::render(frame, list_area, app);

    if let Some(area) = detail_area {
        detail_panel::render(frame, area, app);
    }

    status_bar::render(frame, status_area, app);
    render_key_hints(frame, keys_area, app);

    if app.show_help {
        render_help_overlay(frame, area);
    }

    if let Some(prompt) = &app.password_prompt {
        password_prompt::render(frame, area, prompt);
    }

    #[cfg(feature = "mobile")]
    if let Some(prompt) = &app.pin_unlock_prompt {
        pin_prompt::render(frame, area, prompt);
    }

    if let Some(prompt) = &app.hidden_network_prompt {
        hidden_network_prompt::render(frame, area, prompt);
    }

    if let Some(menu) = &app.add_connection_menu {
        add_connection_menu::render(frame, area, menu);
    }

    if let Some(menu) = &app.vpn_add_menu {
        vpn_add_menu::render(frame, area, menu);
    }

    if let Some(prompt) = &app.vpn_import_prompt {
        vpn_import_prompt::render(frame, area, prompt);
    }

    if let Some(editor) = &app.connection_editor {
        connection_editor::render(frame, area, editor);
    }

    if let Some(prompt) = &app.delete_confirm_prompt {
        delete_confirm_prompt::render(frame, area, prompt);
    }
}

// ── Key hints bar ─────────────────────────────────────────────────────────────

fn render_key_hints(frame: &mut Frame, area: Rect, app: &App) {
    let hints = vec![
        ("↑↓/jk", "Navigate"),
        ("Enter", "Connect"),
        ("d", "Disconnect"),
        ("D", "Delete"),
        ("e", "Edit"),
        ("a", "Add"),
        ("r", "Scan"),
        ("n", "Net"),
        ("w", "Wi-Fi"),
        ("Tab", "Detail"),
        ("?", "Help"),
        ("q", "Quit"),
    ];

    let mut spans: Vec<Span> = Vec::new();
    for (key, desc) in hints {
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            key,
            Style::default().fg(FG_ACCENT).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(desc, Style::default().fg(FG_DIM)));
    }

    if app.demo_mode {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            "[DEMO]",
            Style::default().fg(FG_WARN).add_modifier(Modifier::BOLD),
        ));
    }

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}

// ── Help overlay ──────────────────────────────────────────────────────────────

fn render_help_overlay(frame: &mut Frame, area: Rect) {
    let width = 52u16.min(area.width.saturating_sub(4));
    let height = 20u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let overlay = Rect::new(x, y, width, height);

    let help_lines = vec![
        Line::from(Span::styled(
            "  netman — Help",
            Style::default().fg(FG_ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            "  Navigation",
            Style::default().add_modifier(Modifier::UNDERLINED),
        )),
        Line::raw("  ↑ / k       Move selection up"),
        Line::raw("  ↓ / j       Move selection down"),
        Line::raw(""),
        Line::from(Span::styled(
            "  Connection",
            Style::default().add_modifier(Modifier::UNDERLINED),
        )),
        Line::raw("  Enter       Connect to selected network"),
        Line::raw("  d / Del     Disconnect selected network"),
        Line::raw("  D           Delete selected saved profile"),
        Line::raw("  e           Edit selected saved profile"),
        Line::raw("  a           Add new connection"),
        Line::raw("  r / F5      Scan for Wi-Fi networks"),
        Line::raw("  n           Toggle networking on/off"),
        Line::raw("  w           Toggle Wi-Fi radio on/off"),
        Line::raw(""),
        Line::from(Span::styled(
            "  View",
            Style::default().add_modifier(Modifier::UNDERLINED),
        )),
        Line::raw("  Tab / p     Toggle detail panel"),
        Line::raw("  ?           Toggle this help"),
        Line::raw("  Esc         Close overlay"),
        Line::raw(""),
        Line::from(Span::styled(
            "  General",
            Style::default().add_modifier(Modifier::UNDERLINED),
        )),
        Line::raw("  q           Quit netman"),
        Line::raw("  Ctrl+C      Force quit"),
    ];

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(help_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help ")
                .border_style(Style::default().fg(FG_ACCENT)),
        ),
        overlay,
    );
}
