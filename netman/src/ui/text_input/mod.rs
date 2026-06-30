// SPDX-License-Identifier: GPL-3.0-only

//! Single-line text input with cursor movement and editing.

use crossterm::event::{KeyCode, KeyModifiers};

/// Editable single-line text field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInput {
    text: String,
    cursor: usize,
}

impl TextInput {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn cursor_char_index(&self) -> usize {
        self.text[..self.cursor].chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn insert_str(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    /// Render text, masking characters when `revealed` is false.
    pub fn display_text(&self, revealed: bool) -> String {
        if revealed {
            self.text.clone()
        } else {
            "•".repeat(self.text.chars().count())
        }
    }

    /// Build a one-line ratatui row for this input field.
    pub fn render_line(&self, revealed: bool, width: u16) -> ratatui::text::Line<'static> {
        use ratatui::style::{Modifier, Style};
        use ratatui::text::{Line, Span};

        let field_width = width.saturating_sub(6) as usize;
        let display = self.display_text(revealed);
        let truncated = truncate_display(&display, field_width);
        let cursor_col = self.cursor_char_index().min(field_width.saturating_sub(1));

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

    /// Handle a key event. Returns `true` if the key was consumed.
    pub fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        if modifiers.contains(KeyModifiers::CONTROL) {
            return false;
        }

        match code {
            KeyCode::Char(ch) => {
                self.text.insert(self.cursor, ch);
                self.cursor += ch.len_utf8();
                true
            }
            KeyCode::Backspace => {
                if self.cursor == 0 {
                    return true;
                }
                let prev = prev_char_boundary(&self.text, self.cursor);
                self.text.replace_range(prev..self.cursor, "");
                self.cursor = prev;
                true
            }
            KeyCode::Delete => {
                if self.cursor >= self.text.len() {
                    return true;
                }
                let next = next_char_boundary(&self.text, self.cursor);
                self.text.replace_range(self.cursor..next, "");
                true
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor = prev_char_boundary(&self.text, self.cursor);
                }
                true
            }
            KeyCode::Right => {
                if self.cursor < self.text.len() {
                    self.cursor = next_char_boundary(&self.text, self.cursor);
                }
                true
            }
            KeyCode::Home => {
                self.cursor = 0;
                true
            }
            KeyCode::End => {
                self.cursor = self.text.len();
                true
            }
            _ => false,
        }
    }
}

fn prev_char_boundary(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .char_indices()
        .next_back()
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

fn next_char_boundary(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .char_indices()
        .nth(1)
        .map(|(offset, _)| cursor + offset)
        .unwrap_or(text.len())
}

fn truncate_display(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        text.to_owned()
    } else {
        chars[..max_chars].iter().collect()
    }
}

#[cfg(test)]
mod tests;
