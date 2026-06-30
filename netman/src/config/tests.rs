use super::*;

#[test]
fn load_returns_none_for_nonexistent_file() {
    let result = load(Some(Path::new("/nonexistent/path/netman.toml")));
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn load_returns_none_when_path_is_none_and_no_xdg_file() {
    // Point XDG_CONFIG_HOME somewhere that definitely has no netman config.
    // SAFETY: single-threaded test; no other thread reads the env variable.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/netman-test-no-such-dir-xdg");
    }
    let result = load(None);
    // SAFETY: restoring; see above.
    unsafe {
        std::env::remove_var("XDG_CONFIG_HOME");
    }
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn load_parses_full_config() {
    use std::io::Write;
    let mut f = tempfile::NamedTempFile::new().unwrap();
    writeln!(
        f,
        r#"
[log]
level = "debug"
file = "/tmp/netman.log"

[ui]
tick_rate = 500
show_detail = true
list_height = 20

[network]
auto_scan = true
scan_interval = 30
"#
    )
    .unwrap();

    let cfg = load(Some(f.path())).unwrap().unwrap();
    assert_eq!(cfg.log.level.as_deref(), Some("debug"));
    assert_eq!(cfg.ui.tick_rate, Some(500));
    assert!(cfg.ui.show_detail.unwrap());
    assert!(cfg.network.auto_scan.unwrap());
    assert_eq!(cfg.network.scan_interval, Some(30));
}

#[test]
fn load_parses_empty_toml() {
    use std::io::Write;
    let mut f = tempfile::NamedTempFile::new().unwrap();
    writeln!(f).unwrap();
    let cfg = load(Some(f.path())).unwrap().unwrap();
    assert!(cfg.log.level.is_none());
    assert!(cfg.ui.tick_rate.is_none());
}

#[test]
fn load_rejects_unknown_fields() {
    use std::io::Write;
    let mut f = tempfile::NamedTempFile::new().unwrap();
    writeln!(f, "unknown_top_level_key = true").unwrap();
    let result = load(Some(f.path()));
    assert!(result.is_err());
}
