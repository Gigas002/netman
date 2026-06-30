// Integration tests for netman.
//
// These tests exercise the settings pipeline end-to-end but stop short of
// spawning a terminal or connecting to NetworkManager.

use clap::Parser;
use netman::{cli, config, settings};

fn cli_opts(args: &[&str]) -> cli::CliOptions {
    cli::CliOptions::parse_from(std::iter::once("netman").chain(args.iter().copied()))
}

#[test]
fn full_settings_pipeline_no_config() {
    let opts = cli_opts(&["-v"]);
    let cfg = config::load(None).expect("load should not fail");
    let settings = settings::resolve(opts, cfg);
    assert_eq!(settings.log_level, "debug");
    assert_eq!(settings.tick_rate, 1000);
}

#[test]
fn full_settings_pipeline_with_config_file() {
    use std::io::Write;
    let mut f = tempfile::NamedTempFile::new().unwrap();
    writeln!(
        f,
        r#"
[ui]
tick_rate = 250
show_detail = false

[network]
scan_interval = 60
"#
    )
    .unwrap();

    let opts = cli_opts(&[]);
    let cfg = config::load(Some(f.path()))
        .expect("load should not fail")
        .expect("config should be present");
    let settings = settings::resolve(opts, Some(cfg));

    assert_eq!(settings.tick_rate, 250);
    assert!(!settings.show_detail);
    assert_eq!(settings.scan_interval, 60);
}
