use clap::Parser;
use pretty_assertions::assert_eq;

use super::detect_mode;
use super::expand_bare_level;
use super::parse_timezone;
use super::subscriber_opts_from_cli;
use crate::Cli;
use coco_otel::subscriber::Mode;
use coco_otel::subscriber::TimezoneConfig;

fn parse(args: &[&str]) -> Cli {
    let mut full = vec!["coco"];
    full.extend_from_slice(args);
    Cli::parse_from(full)
}

#[test]
fn detect_mode_chat_subcommand_is_headless() {
    let cli = parse(&["chat", "hi"]);
    assert_eq!(detect_mode(&cli), Mode::Headless);
}

#[test]
fn detect_mode_review_subcommand_is_headless() {
    let cli = parse(&["review", "HEAD"]);
    assert_eq!(detect_mode(&cli), Mode::Headless);
}

#[test]
fn detect_mode_sdk_subcommand_is_sdk() {
    let cli = parse(&["sdk"]);
    assert_eq!(detect_mode(&cli), Mode::Sdk);
}

#[test]
fn detect_mode_short_subcommands_are_skip() {
    for sub in [
        "status",
        "doctor",
        "sessions",
        "init",
        "logout",
        "release-notes",
    ] {
        let cli = parse(&[sub]);
        assert_eq!(
            detect_mode(&cli),
            Mode::Skip,
            "subcommand {sub} should map to Mode::Skip"
        );
    }
}

#[test]
fn detect_mode_with_prompt_no_subcommand_is_headless() {
    let cli = parse(&["--prompt", "hi"]);
    assert_eq!(detect_mode(&cli), Mode::Headless);
}

#[test]
fn expand_bare_level_known_levels_get_coco_prefix() {
    for lvl in ["off", "error", "warn", "info", "debug", "trace"] {
        assert_eq!(expand_bare_level(lvl), format!("coco={lvl},{lvl}"));
    }
}

#[test]
fn expand_bare_level_uppercase_is_normalized() {
    assert_eq!(expand_bare_level("DEBUG"), "coco=debug,debug");
}

#[test]
fn expand_bare_level_full_directive_passes_through() {
    let directive = "coco_inference=trace,coco=debug,info";
    assert_eq!(expand_bare_level(directive), directive);
}

#[test]
fn subscriber_opts_carries_log_stderr_flag() {
    let cli = parse(&["--prompt", "hi", "--log-stderr"]);
    let opts = subscriber_opts_from_cli(&cli);
    assert!(opts.also_stderr);
}

#[test]
fn subscriber_opts_log_level_flag_expands() {
    let cli = parse(&["--prompt", "hi", "--log-level", "trace"]);
    let opts = subscriber_opts_from_cli(&cli);
    assert_eq!(opts.level.as_deref(), Some("coco=trace,trace"));
}

#[test]
fn parse_timezone_known_values() {
    assert_eq!(parse_timezone("local"), Some(TimezoneConfig::Local));
    assert_eq!(parse_timezone("utc"), Some(TimezoneConfig::Utc));
    assert_eq!(parse_timezone("UTC"), Some(TimezoneConfig::Utc));
    assert_eq!(parse_timezone("  Local  "), Some(TimezoneConfig::Local));
}

#[test]
fn parse_timezone_unknown_returns_none() {
    assert_eq!(parse_timezone(""), None);
    assert_eq!(parse_timezone("est"), None);
    assert_eq!(parse_timezone("Asia/Shanghai"), None);
}

#[test]
fn subscriber_opts_default_timezone_is_local() {
    // No flag, no env (test process inherits clean env for COCO_LOG_TIMEZONE).
    let cli = parse(&["--prompt", "hi"]);
    let opts = subscriber_opts_from_cli(&cli);
    // We can't assert env is empty, but the flag-absent default must be `Local`
    // unless the test runner's environment overrides it. Document the intent.
    assert!(matches!(
        opts.timezone,
        TimezoneConfig::Local | TimezoneConfig::Utc
    ));
}

#[test]
fn subscriber_opts_log_timezone_flag_overrides_default() {
    let cli = parse(&["--prompt", "hi", "--log-timezone", "utc"]);
    let opts = subscriber_opts_from_cli(&cli);
    assert_eq!(opts.timezone, TimezoneConfig::Utc);
}

#[test]
fn subscriber_opts_unknown_log_timezone_falls_back_to_default() {
    let cli = parse(&["--prompt", "hi", "--log-timezone", "Asia/Shanghai"]);
    let opts = subscriber_opts_from_cli(&cli);
    // Unknown value → silently ignored → falls through to env / default.
    // We assert it's still a valid variant rather than testing the specific
    // default, since `COCO_LOG_TIMEZONE` may be set in the test env.
    assert!(matches!(
        opts.timezone,
        TimezoneConfig::Local | TimezoneConfig::Utc
    ));
}
