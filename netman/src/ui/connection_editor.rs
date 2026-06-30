// SPDX-License-Identifier: GPL-3.0-only

//! Connection profile editor modal (multi-field form).

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::{ConnectionEditor, EditorFieldId},
    ui::{FG_ACCENT, FG_DIM, FG_WARN},
};

/// Render the connection editor overlay when active.
pub fn render(frame: &mut Frame, area: Rect, editor: &ConnectionEditor) {
    let field_count = editor.fields.len();
    let width = 58u16.min(area.width.saturating_sub(4));
    let height = (field_count as u16 * 2 + 10).min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let overlay = Rect::new(x, y, width, height);

    let mut lines = vec![Line::raw("")];

    for (idx, field) in editor.fields.iter().enumerate() {
        let focused = idx == editor.focused;
        lines.push(field_label(field.label(), focused));
        lines.push(field_value(editor, *field, focused, width));
    }

    if let Some(err) = &editor.error {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            format!("  {err}"),
            Style::default().fg(FG_WARN),
        )));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("  Tab", Style::default().fg(FG_ACCENT)),
        Span::raw(" next   "),
        Span::styled("Enter", Style::default().fg(FG_ACCENT)),
        Span::raw(" save   "),
        Span::styled("Esc", Style::default().fg(FG_ACCENT)),
        Span::raw(" cancel"),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  ←/→", Style::default().fg(FG_ACCENT)),
        Span::raw(" change choice   "),
        Span::styled("Ctrl-H", Style::default().fg(FG_ACCENT)),
        Span::raw(" show/hide secret"),
    ]));

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(
                    " {}: {} ",
                    if editor.is_new() { "Add" } else { "Edit" },
                    editor.title
                ))
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

fn field_value(
    editor: &ConnectionEditor,
    field: EditorFieldId,
    focused: bool,
    width: u16,
) -> Line<'static> {
    if field.is_text(editor.is_new()) {
        let input = editor.inputs.get(&field);
        let secret = field.is_secret();
        if let Some(input) = input {
            return input.render_line(!secret || editor.show_secrets, width);
        }
    }

    let value = editor.display_value(field);
    let style = if focused {
        Style::default()
            .fg(FG_ACCENT)
            .add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    Line::from(Span::styled(format!("  {value}"), style))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::editor_fields_for;
    use libnetman::connection::{ConnectionProfile, Ipv4Profile, WifiProfile, WifiSecurity};

    #[test]
    fn wifi_editor_has_expected_fields() {
        let profile = ConnectionProfile::Wifi(WifiProfile {
            ssid: "x".into(),
            security: WifiSecurity::Wpa2,
            psk: String::new(),
            hidden: false,
            ipv4: Ipv4Profile::default(),
        });
        let fields = editor_fields_for(&profile, false);
        assert!(fields.contains(&EditorFieldId::Ssid));
        assert!(fields.contains(&EditorFieldId::IpMethod));
    }
}
