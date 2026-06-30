// SPDX-License-Identifier: GPL-3.0-only

//! Delete-connection confirmation modal.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::DeleteConfirmPrompt,
    ui::{FG_ACCENT, FG_DIM, FG_WARN},
};

/// Render the delete-confirmation overlay when active.
pub fn render(frame: &mut Frame, area: Rect, prompt: &DeleteConfirmPrompt) {
    let width = 54u16.min(area.width.saturating_sub(4));
    let height = 11u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let overlay = Rect::new(x, y, width, height);

    let mut lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled("  Delete ", Style::default().fg(FG_WARN)),
            Span::styled(
                &prompt.label,
                Style::default().fg(FG_WARN).add_modifier(Modifier::BOLD),
            ),
            Span::raw("?"),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "  This removes the saved profile permanently.",
            Style::default().fg(FG_DIM),
        )),
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
        Span::raw(" delete   "),
        Span::styled("Esc", Style::default().fg(FG_ACCENT)),
        Span::raw(" cancel"),
    ]));

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Confirm delete ")
                .border_style(Style::default().fg(FG_WARN)),
        ),
        overlay,
    );
}
