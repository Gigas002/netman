// UI rendering tests: verify key helper functions produce expected output
// without requiring a live terminal (no frame rendering here).

use libnetman::connection::WifiInfo;
use libnetman::connection::{WifiMode, WifiSecurity};

#[test]
fn strength_bar_boundaries() {
    let make_wifi = |strength| WifiInfo {
        ssid: "test".into(),
        strength,
        security: WifiSecurity::None,
        frequency: None,
        bssid: None,
        mode: WifiMode::Infrastructure,
    };

    let full = make_wifi(100);
    let empty = make_wifi(0);
    assert_eq!(full.strength_bar(), "████");
    assert_eq!(empty.strength_bar(), "░░░░");

    for s in [25u8, 50, 75, 99] {
        let w = make_wifi(s);
        let bar = w.strength_bar();
        assert_eq!(bar.chars().count(), 4, "bar must be 4 chars wide at {s}%");
    }
}

fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
    buffer.content().iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn help_overlay_shows_every_line_on_a_normal_terminal() {
    let backend = ratatui::backend::TestBackend::new(80, 40);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| super::render_help_overlay(frame, frame.area()))
        .unwrap();

    // The last section was cut off by a fixed 20-row box.
    let text = buffer_text(terminal.backend().buffer());
    assert!(text.contains("Force quit"));
}

#[test]
fn help_overlay_fits_a_tiny_terminal() {
    for (w, h) in [(10, 3), (20, 10), (1, 1), (0, 0)] {
        let backend = ratatui::backend::TestBackend::new(w, h);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| super::render_help_overlay(frame, frame.area()))
            .unwrap();
    }
}

fn draw_hidden_prompt(prompt: &crate::app::HiddenNetworkPrompt) -> String {
    let backend = ratatui::backend::TestBackend::new(80, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| super::hidden_network_prompt::render(frame, frame.area(), prompt))
        .unwrap();
    buffer_text(terminal.backend().buffer())
}

#[test]
fn hidden_prompt_shows_security_and_every_line() {
    let mut prompt = crate::app::HiddenNetworkPrompt::new();
    prompt.error = Some("WPA2 needs a password".into());
    let text = draw_hidden_prompt(&prompt);
    assert!(text.contains("Security"));
    assert!(text.contains("WPA2"));
    assert!(text.contains("Password"));
    // The last hint line fits inside the box with an error showing.
    assert!(text.contains("show/hide password"));

    prompt.security = WifiSecurity::Owe;
    let text = draw_hidden_prompt(&prompt);
    assert!(text.contains("OWE"));
    assert!(text.contains("No password"));
}
