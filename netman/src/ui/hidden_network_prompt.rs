// SPDX-License-Identifier: GPL-3.0-only

//! Hidden Wi-Fi network connection modal (SSID + password).

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::{HiddenNetworkPrompt, HiddenPromptField},
    ui::{FG_ACCENT, FG_DIM, FG_WARN},
};

/// Render the hidden-network overlay when active.
pub fn render(frame: &mut Frame, area: Rect, prompt: &HiddenNetworkPrompt) {
    let width = 54u16.min(area.width.saturating_sub(4));
    let height = 13u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let overlay = Rect::new(x, y, width, height);

    let ssid_label = field_label("SSID", prompt.focused == HiddenPromptField::Ssid);
    let password_label = field_label("Password", prompt.focused == HiddenPromptField::Password);

    let mut lines = vec![
        Line::raw(""),
        ssid_label,
        prompt.ssid.render_line(true, width),
        Line::raw(""),
        password_label,
        prompt.password.render_line(prompt.show_password, width),
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
        Span::styled("  Tab", Style::default().fg(FG_ACCENT)),
        Span::raw(" next field   "),
        Span::styled("Enter", Style::default().fg(FG_ACCENT)),
        Span::raw(" connect"),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Esc", Style::default().fg(FG_ACCENT)),
        Span::raw(" cancel   "),
        Span::styled("Ctrl-H", Style::default().fg(FG_ACCENT)),
        Span::raw(" show/hide password"),
    ]));

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Hidden Wi-Fi ")
                .border_style(Style::default().fg(FG_ACCENT)),
        ),
        overlay,
    );
}

fn field_label(label: &str, focused: bool) -> Line<'static> {
    let style = if focused {
        Style::default()
            .fg(FG_ACCENT)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::default().fg(FG_DIM)
    };
    Line::from(Span::styled(format!("  {label}"), style))
}
