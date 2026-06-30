// SPDX-License-Identifier: GPL-3.0-only

//! Add-connection type picker (Wi-Fi / Ethernet / VPN).

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::{AddConnectionMenu, NewConnectionKind},
    ui::{FG_ACCENT, FG_DIM},
};

/// Render the add-connection type menu when active.
pub fn render(frame: &mut Frame, area: Rect, menu: &AddConnectionMenu) {
    let width = 44u16.min(area.width.saturating_sub(4));
    let height = 12u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let overlay = Rect::new(x, y, width, height);

    let kinds = NewConnectionKind::all();
    let mut lines = vec![
        Line::raw(""),
        Line::from(Span::styled(
            "  Choose connection type:",
            Style::default().fg(FG_DIM),
        )),
        Line::raw(""),
    ];

    for (idx, kind) in kinds.iter().enumerate() {
        let focused = idx == menu.selected;
        let style = if focused {
            Style::default()
                .fg(FG_ACCENT)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!("  {} {}", if focused { ">" } else { " " }, kind.label()),
            style,
        )));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("  ↑↓/jk", Style::default().fg(FG_ACCENT)),
        Span::raw(" navigate   "),
        Span::styled("Enter", Style::default().fg(FG_ACCENT)),
        Span::raw(" select"),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Esc", Style::default().fg(FG_ACCENT)),
        Span::raw(" cancel"),
    ]));

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Add connection ")
                .border_style(Style::default().fg(FG_ACCENT)),
        ),
        overlay,
    );
}
