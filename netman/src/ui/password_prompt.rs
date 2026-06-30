// SPDX-License-Identifier: GPL-3.0-only

//! Wi-Fi password modal overlay.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::PasswordPrompt,
    ui::{FG_ACCENT, FG_DIM, FG_WARN, TextInput},
};

/// Render the password prompt overlay when active.
pub fn render(frame: &mut Frame, area: Rect, prompt: &PasswordPrompt) {
    let width = 54u16.min(area.width.saturating_sub(4));
    let height = 11u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let overlay = Rect::new(x, y, width, height);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  Network: ", Style::default().fg(FG_DIM)),
            Span::raw(prompt.ssid.clone()),
        ]),
        Line::from(vec![
            Span::styled("  Security: ", Style::default().fg(FG_DIM)),
            Span::raw(prompt.security.label()),
        ]),
        Line::raw(""),
        Line::from(Span::styled("  Password", Style::default().fg(FG_DIM))),
        render_input_line(&prompt.input, prompt.show_password, width),
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
        Span::raw(" connect   "),
        Span::styled("Esc", Style::default().fg(FG_ACCENT)),
        Span::raw(" cancel   "),
        Span::styled("Ctrl-H", Style::default().fg(FG_ACCENT)),
        Span::raw(" show/hide"),
    ]));

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Connect to Wi-Fi ")
                .border_style(Style::default().fg(FG_ACCENT)),
        ),
        overlay,
    );
}

fn render_input_line(input: &TextInput, revealed: bool, width: u16) -> Line<'static> {
    let field_width = width.saturating_sub(6) as usize;
    let display = input.display_text(revealed);
    let truncated = truncate_display(&display, field_width);
    let cursor_col = input.cursor_char_index().min(field_width.saturating_sub(1));

    let mut spans = vec![Span::raw("  ")];
    for (idx, ch) in truncated.chars().enumerate() {
        let style = if idx == cursor_col {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        spans.push(Span::styled(ch.to_string(), style));
    }

    if truncated.is_empty() {
        spans.push(Span::styled(
            " ",
            Style::default().add_modifier(Modifier::REVERSED),
        ));
    }

    Line::from(spans)
}

fn truncate_display(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        text.to_owned()
    } else {
        chars[..max_chars].iter().collect()
    }
}
