// SPDX-License-Identifier: GPL-3.0-only

//! VPN configuration file import modal.

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::VpnImportPrompt,
    ui::{FG_ACCENT, FG_DIM, FG_WARN},
};

/// Render the VPN import overlay when active.
pub fn render(frame: &mut Frame, area: Rect, prompt: &VpnImportPrompt) {
    let width = 58u16.min(area.width.saturating_sub(4));
    let height = 14u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let overlay = Rect::new(x, y, width, height);

    let plugin = prompt
        .plugins
        .get(prompt.selected_plugin)
        .map(|p| p.label.as_str())
        .unwrap_or("?");

    let activate = if prompt.activate_on_save { "yes" } else { "no" };

    let mut lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled("  Plugin (←/→): ", Style::default().fg(FG_DIM)),
            Span::styled(plugin, Style::default().fg(FG_ACCENT)),
        ]),
        Line::raw(""),
        Line::from(Span::styled("  File path", Style::default().fg(FG_DIM))),
        prompt.path.render_line(true, width),
        Line::raw(""),
        Line::from(vec![
            Span::styled("  Activate after import: ", Style::default().fg(FG_DIM)),
            Span::raw(activate),
        ]),
    ];

    if let Some(err) = &prompt.error {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            format!("  {err}"),
            Style::default().fg(FG_WARN),
        )));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("  ←/→", Style::default().fg(FG_ACCENT)),
        Span::raw(" plugin   "),
        Span::styled("Enter", Style::default().fg(FG_ACCENT)),
        Span::raw(" import"),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Space", Style::default().fg(FG_ACCENT)),
        Span::raw(" toggle activate   "),
        Span::styled("Esc", Style::default().fg(FG_ACCENT)),
        Span::raw(" cancel"),
    ]));

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Import VPN ")
                .border_style(Style::default().fg(FG_ACCENT)),
        ),
        overlay,
    );
}
