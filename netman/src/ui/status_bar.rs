// SPDX-License-Identifier: GPL-3.0-only

//! Status bar — one-line summary rendered below the main body.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use libnetman::connection::{ConnectionStatus, NmState};

use crate::{
    app::App,
    ui::{FG_ACTIVE, FG_DIM, FG_WARN},
};

/// Render the status bar into `area`.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let line = if let Some(msg) = &app.status_message {
        Line::from(Span::styled(
            format!(" {msg}"),
            Style::default().fg(FG_WARN),
        ))
    } else {
        build_status_line(app)
    };

    frame.render_widget(Paragraph::new(line), area);
}

fn build_status_line(app: &App) -> Line<'static> {
    let state_color = if app.nm_state.is_connected() {
        FG_ACTIVE
    } else {
        FG_WARN
    };

    let indicator = match app.nm_state {
        NmState::ConnectedGlobal | NmState::ConnectedSite | NmState::ConnectedLocal => '●',
        NmState::Connecting => '◌',
        NmState::Disconnected | NmState::Disconnecting => '○',
        _ => '?',
    };

    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            indicator.to_string(),
            Style::default()
                .fg(state_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            app.nm_state.label().to_owned(),
            Style::default().fg(state_color),
        ),
    ];

    // Append active connection summary.
    let active = app
        .items
        .iter()
        .filter_map(|i| i.as_connection())
        .find(|c| matches!(c.status, ConnectionStatus::Active));

    if let Some(conn) = active {
        spans.push(Span::styled("  ─  ", Style::default().fg(FG_DIM)));
        spans.push(Span::raw(conn.label().to_owned()));

        if let Some(dev) = &conn.device {
            spans.push(Span::styled(
                format!(" [{dev}]"),
                Style::default().fg(FG_DIM),
            ));
        }

        if let Some(ip4) = &conn.ip4 {
            let addr = ip4.address.split('/').next().unwrap_or(&ip4.address);
            spans.push(Span::styled(
                format!("  {addr}"),
                Style::default().fg(FG_DIM),
            ));
        }
    }

    if !app.networking_enabled {
        spans.push(Span::styled("  ─  ", Style::default().fg(FG_DIM)));
        spans.push(Span::styled(
            "Net: off",
            Style::default().fg(FG_WARN).add_modifier(Modifier::BOLD),
        ));
    }
    if !app.wireless_enabled {
        spans.push(Span::styled("  ", Style::default()));
        spans.push(Span::styled(
            "Wi-Fi: off",
            Style::default().fg(FG_WARN).add_modifier(Modifier::BOLD),
        ));
    }

    Line::from(spans)
}
