// SPDX-License-Identifier: GPL-3.0-only

//! SIM PIN unlock modal overlay.

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::PinUnlockPrompt,
    ui::{FG_ACCENT, FG_DIM, FG_WARN},
};

/// Render the SIM PIN unlock overlay when active.
pub fn render(frame: &mut Frame, area: Rect, prompt: &PinUnlockPrompt) {
    let width = 54u16.min(area.width.saturating_sub(4));
    let height = 11u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let overlay = Rect::new(x, y, width, height);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  Modem: ", Style::default().fg(FG_DIM)),
            Span::raw(prompt.label.clone()),
        ]),
        Line::raw(""),
        Line::from(Span::styled("  SIM PIN", Style::default().fg(FG_DIM))),
        prompt.input.render_line(false, width),
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
        Span::styled("  Enter", Style::default().fg(FG_ACCENT)),
        Span::raw(" unlock   "),
        Span::styled("Esc", Style::default().fg(FG_ACCENT)),
        Span::raw(" cancel"),
    ]));

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Unlock SIM ")
                .border_style(Style::default().fg(FG_ACCENT)),
        ),
        overlay,
    );
}
