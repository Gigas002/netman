use super::*;
use clap::Parser;

fn parse_args(args: &[&str]) -> CliOptions {
    CliOptions::parse_from(std::iter::once("netman").chain(args.iter().copied()))
}

#[test]
fn defaults_are_none() {
    let opts = parse_args(&[]);
    assert!(opts.config.is_none());
    assert_eq!(opts.verbose, 0);
    assert!(opts.log_file.is_none());
    assert_eq!(opts.tick_rate, 1000);
}

#[test]
fn config_short_flag() {
    let opts = parse_args(&["-c", "/tmp/netman.toml"]);
    assert_eq!(opts.config.unwrap().to_str().unwrap(), "/tmp/netman.toml");
}

#[test]
fn config_long_flag() {
    let opts = parse_args(&["--config", "/tmp/netman.toml"]);
    assert_eq!(opts.config.unwrap().to_str().unwrap(), "/tmp/netman.toml");
}

#[test]
fn verbose_count() {
    let opts = parse_args(&["-vv"]);
    assert_eq!(opts.verbose, 2);
}

#[test]
fn tick_rate_override() {
    let opts = parse_args(&["--tick-rate", "500"]);
    assert_eq!(opts.tick_rate, 500);
}

#[test]
fn log_file_long() {
    let opts = parse_args(&["--log-file", "/var/log/netman.log"]);
    assert_eq!(
        opts.log_file.unwrap().to_str().unwrap(),
        "/var/log/netman.log"
    );
}
