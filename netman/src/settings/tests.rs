use clap::Parser;

use super::*;
use crate::{cli::CliOptions, config::FileConfig};

fn cli(args: &[&str]) -> CliOptions {
    CliOptions::parse_from(std::iter::once("netman").chain(args.iter().copied()))
}

fn empty_config() -> Option<FileConfig> {
    None
}

#[test]
fn defaults_when_no_args_and_no_config() {
    let s = resolve(cli(&[]), empty_config());
    assert_eq!(s.log_level, "warn");
    assert!(s.log_file.is_none());
    assert_eq!(s.tick_rate, 1000);
    assert!(s.show_detail);
    assert!(s.auto_scan);
    assert_eq!(s.scan_interval, 30);
}

#[test]
fn verbose_flag_overrides_log_level() {
    let s = resolve(cli(&["-v"]), empty_config());
    assert_eq!(s.log_level, "debug");

    let s = resolve(cli(&["-vv"]), empty_config());
    assert_eq!(s.log_level, "trace");
}

#[test]
fn config_file_log_level_beats_default() {
    let mut cfg = FileConfig::default();
    cfg.log.level = Some("info".into());
    let s = resolve(cli(&[]), Some(cfg));
    assert_eq!(s.log_level, "info");
}

#[test]
fn cli_verbose_beats_config_log_level() {
    let mut cfg = FileConfig::default();
    cfg.log.level = Some("info".into());
    let s = resolve(cli(&["-v"]), Some(cfg));
    assert_eq!(s.log_level, "debug");
}

#[test]
fn cli_tick_rate_not_1000_beats_config() {
    let mut cfg = FileConfig::default();
    cfg.ui.tick_rate = Some(200);
    let s = resolve(cli(&["--tick-rate", "500"]), Some(cfg));
    assert_eq!(s.tick_rate, 500);
}

#[test]
fn config_tick_rate_beats_default_when_cli_is_default() {
    let mut cfg = FileConfig::default();
    cfg.ui.tick_rate = Some(250);
    let s = resolve(cli(&[]), Some(cfg));
    assert_eq!(s.tick_rate, 250);
}

#[test]
fn log_file_from_cli() {
    let s = resolve(cli(&["--log-file", "/tmp/netman.log"]), empty_config());
    assert_eq!(s.log_file.unwrap().to_str().unwrap(), "/tmp/netman.log");
}

#[test]
fn log_file_from_config_when_no_cli() {
    let mut cfg = FileConfig::default();
    cfg.log.file = Some("/tmp/cfg.log".into());
    let s = resolve(cli(&[]), Some(cfg));
    assert_eq!(s.log_file.unwrap().to_str().unwrap(), "/tmp/cfg.log");
}
