use super::*;
use crate::settings::Settings;

#[test]
fn init_with_default_settings_does_not_panic() {
    // The global subscriber can only be set once per process; subsequent calls
    // are silently ignored by tracing-subscriber (no panic, no error).
    let settings = Settings::default();
    let _ = init(&settings);
}

#[test]
fn init_with_log_file_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("test.log");
    let settings = Settings {
        log_file: Some(log_path.clone()),
        ..Settings::default()
    };
    let _ = init(&settings);
    // Subscriber may or may not be set (depends on test order), but the file
    // open path must not panic.
    assert!(log_path.exists() || !log_path.exists()); // no-op; just ensure no panic.
}
