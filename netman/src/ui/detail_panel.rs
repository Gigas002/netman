// SPDX-License-Identifier: GPL-3.0-only

//! Detail panel — shows extended info for the currently selected connection.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use libnetman::connection::{ConnectionKind, ConnectionStatus};

use crate::{
    app::App,
    ui::{FG_ACCENT, FG_ACTIVE, FG_DIM, FG_WARN},
};

/// Render the detail panel for the selected connection.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let lines = match app.selected_conn() {
        Some(conn) => build_lines(conn),
        None => vec![Line::styled(
            "  No connection selected",
            Style::default().fg(FG_DIM),
        )],
    };

    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Details ")
                .border_style(Style::default().fg(FG_ACCENT)),
        ),
        area,
    );
}

fn build_lines(conn: &libnetman::connection::Connection) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();

    let status_color = match conn.status {
        ConnectionStatus::Active => FG_ACTIVE,
        ConnectionStatus::Activating | ConnectionStatus::Deactivating => FG_WARN,
        _ => FG_DIM,
    };

    // Header
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            conn.status.indicator().to_string(),
            Style::default().fg(status_color),
        ),
        Span::raw(" "),
        Span::styled(
            conn.label().to_owned(),
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::raw(""));

    // Type-specific info
    match &conn.kind {
        ConnectionKind::Wifi(wifi) => {
            field(&mut lines, "Type", "Wi-Fi");
            field(&mut lines, "SSID", &wifi.ssid);
            field(&mut lines, "Security", wifi.security.label());
            field(
                &mut lines,
                "Signal",
                &format!("{}% {}", wifi.strength, wifi.strength_bar()),
            );
            if let Some(band) = wifi.band_label() {
                field(&mut lines, "Band", band);
            }
            if let Some(freq) = wifi.frequency {
                field(&mut lines, "Frequency", &format!("{freq} MHz"));
            }
            if let Some(bssid) = &wifi.bssid {
                field(&mut lines, "BSSID", bssid);
            }
        }
        ConnectionKind::Ethernet => {
            field(&mut lines, "Type", "Ethernet");
        }
        ConnectionKind::Vpn(vpn) => {
            field(&mut lines, "Type", "VPN");
            let short_type = vpn
                .service_type
                .split('.')
                .next_back()
                .unwrap_or(&vpn.service_type);
            field(&mut lines, "Plugin", short_type);
        }
        ConnectionKind::Loopback => {
            field(&mut lines, "Type", "Loopback");
        }
        ConnectionKind::Other(t) => {
            field(&mut lines, "Type", t);
        }
    }

    if let Some(dev) = &conn.device {
        field(&mut lines, "Device", dev);
    }

    // IPv4 section
    if let Some(ip4) = &conn.ip4 {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "  IPv4",
            Style::default()
                .fg(FG_ACCENT)
                .add_modifier(Modifier::UNDERLINED),
        )));
        field(&mut lines, "Address", &ip4.address);
        if let Some(gw) = &ip4.gateway {
            field(&mut lines, "Gateway", gw);
        }
        for (i, ns) in ip4.nameservers.iter().enumerate() {
            if i == 0 {
                field(&mut lines, "DNS", ns);
            } else {
                field(&mut lines, "", ns);
            }
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        format!("  Status: {}", conn.status.label()),
        Style::default().fg(status_color),
    )));

    lines
}

fn field(lines: &mut Vec<Line<'static>>, label: &str, value: &str) {
    lines.push(Line::from(vec![
        Span::styled(format!("  {label:<11} "), Style::default().fg(FG_DIM)),
        Span::raw(value.to_owned()),
    ]));
}
