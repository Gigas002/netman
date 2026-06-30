use crossterm::event::{KeyCode, KeyModifiers};

use super::TextInput;

#[test]
fn insert_and_backspace() {
    let mut input = TextInput::new();
    input.handle_key(KeyCode::Char('a'), KeyModifiers::NONE);
    input.handle_key(KeyCode::Char('b'), KeyModifiers::NONE);
    assert_eq!(input.text(), "ab");
    input.handle_key(KeyCode::Backspace, KeyModifiers::NONE);
    assert_eq!(input.text(), "a");
}

#[test]
fn masked_display() {
    let mut input = TextInput::new();
    input.insert_str("secret");
    assert_eq!(input.display_text(false), "••••••");
    assert_eq!(input.display_text(true), "secret");
}

#[test]
fn insert_str_at_cursor() {
    let mut input = TextInput::new();
    input.insert_str("hi");
    input.handle_key(KeyCode::Home, KeyModifiers::NONE);
    input.insert_str("X");
    assert_eq!(input.text(), "Xhi");
}
