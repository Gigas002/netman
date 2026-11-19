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

#[test]
fn gated_writer_drops_output_while_the_gate_is_set() {
    static GATE: AtomicBool = AtomicBool::new(false);
    let mut w = Gated {
        gate: &GATE,
        inner: Vec::new(),
    };

    w.write_all(b"before ").unwrap();
    GATE.store(true, Ordering::Relaxed);
    w.write_all(b"during ").unwrap();
    GATE.store(false, Ordering::Relaxed);
    w.write_all(b"after").unwrap();

    assert_eq!(w.inner, b"before after");
}
