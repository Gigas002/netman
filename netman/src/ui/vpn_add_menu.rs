// SPDX-License-Identifier: GPL-3.0-only

//! VPN add sub-menu — installed plugins and import entry.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::VpnAddMenu,
    ui::{FG_ACCENT, FG_DIM},
};

/// Render the VPN add sub-menu when active.
pub fn render(frame: &mut Frame, area: Rect, menu: &VpnAddMenu) {
    let item_count = menu.item_count();
    let width = 50u16.min(area.width.saturating_sub(4));
    let height = (item_count as u16 + 9).min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let overlay = Rect::new(x, y, width, height);

    let mut lines = vec![
        Line::raw(""),
        Line::from(Span::styled(
            "  Add VPN connection:",
            Style::default().fg(FG_DIM),
        )),
        Line::raw(""),
    ];

    for idx in 0..item_count {
        let focused = idx == menu.selected;
        let label = menu.item_label(idx);
        let style = if focused {
            Style::default()
                .fg(FG_ACCENT)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!("  {} {label}", if focused { ">" } else { " " }),
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
        Span::raw(" back"),
    ]));

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" VPN ")
                .border_style(Style::default().fg(FG_ACCENT)),
        ),
        overlay,
    );
}
