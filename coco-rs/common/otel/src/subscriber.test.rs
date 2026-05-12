use std::path::PathBuf;

use pretty_assertions::assert_eq;

use super::DEFAULT_FILTER;
use super::Format;
use super::Mode;
use super::SubscriberOpts;
use super::TimezoneConfig;
use super::default_format;
use super::init_for_tests;
use super::init_subscriber;
use super::resolve_log_path;

fn opts(mode: Mode) -> SubscriberOpts {
    SubscriberOpts {
        mode,
        level: None,
        format: None,
        file: None,
        also_stderr: false,
        location: false,
        thread_names: false,
        default_log_dir: PathBuf::from("/tmp/coco-test/logs"),
        default_file_prefix: "coco".to_string(),
        timezone: TimezoneConfig::default(),
    }
}

#[test]
fn format_parse_known_values() {
    assert_eq!(Format::parse("pretty"), Some(Format::Pretty));
    assert_eq!(Format::parse("compact"), Some(Format::Compact));
    assert_eq!(Format::parse("json"), Some(Format::Json));
    assert_eq!(Format::parse("JSON"), Some(Format::Json));
    assert_eq!(Format::parse("  Pretty  "), Some(Format::Pretty));
}

#[test]
fn format_parse_unknown_returns_none() {
    assert_eq!(Format::parse(""), None);
    assert_eq!(Format::parse("yaml"), None);
}

#[test]
fn format_round_trip() {
    for f in [Format::Pretty, Format::Compact, Format::Json] {
        assert_eq!(Format::parse(f.as_str()), Some(f));
    }
}

#[test]
fn default_format_per_mode() {
    assert_eq!(default_format(Mode::Sdk), Format::Json);
    assert_eq!(default_format(Mode::Tui), Format::Compact);
    assert_eq!(default_format(Mode::Headless), Format::Compact);
}

#[test]
fn default_filter_is_dev_friendly() {
    // Guard the documented dev-phase default. If we change this,
    // update the conventions doc in the same commit.
    assert_eq!(DEFAULT_FILTER, "coco=debug,info");
}

#[test]
fn skip_mode_returns_none_without_install() {
    // Mode::Skip must not register a global subscriber — short
    // subcommands rely on this to keep their stdout clean.
    let result = init_subscriber(opts(Mode::Skip)).expect("Skip should not error");
    assert!(result.is_none());
}

#[test]
fn resolve_log_path_uses_explicit_when_set() {
    let mut o = opts(Mode::Tui);
    o.file = Some(PathBuf::from("/var/log/coco/custom.log"));
    assert_eq!(
        resolve_log_path(&o),
        PathBuf::from("/var/log/coco/custom.log")
    );
}

#[test]
fn resolve_log_path_default_uses_dir_and_prefix() {
    let o = SubscriberOpts {
        default_log_dir: PathBuf::from("/home/u/.coco/logs"),
        default_file_prefix: "session".to_string(),
        ..opts(Mode::Headless)
    };
    assert_eq!(
        resolve_log_path(&o),
        PathBuf::from("/home/u/.coco/logs/session.log")
    );
}

#[test]
fn init_for_tests_is_idempotent() {
    // OnceLock guard means double-call is a no-op rather than a panic.
    init_for_tests();
    init_for_tests();
}

#[test]
fn opts_default_timezone_is_local() {
    // Sanity-check the default so downstream tests can rely on it.
    assert_eq!(opts(Mode::Tui).timezone, TimezoneConfig::Local);
}

#[test]
fn skip_mode_ignores_timezone() {
    // Skip path returns early — passing a non-default timezone must
    // not change behavior.
    let mut o = opts(Mode::Skip);
    o.timezone = TimezoneConfig::Utc;
    let result = init_subscriber(o).expect("Skip should not error");
    assert!(result.is_none());
}

#[test]
fn opts_layout_toggles_default_off() {
    // Verbose layout (file:line + thread name) must stay off unless
    // the CLI layer opts in — the doc rationale is the per-event byte
    // cost.
    let o = opts(Mode::Tui);
    assert!(!o.location);
    assert!(!o.thread_names);
}

#[test]
fn skip_mode_ignores_layout_toggles() {
    // Skip path returns early — non-default layout flags must not
    // alter the contract that no subscriber is installed.
    let mut o = opts(Mode::Skip);
    o.location = true;
    o.thread_names = true;
    let result = init_subscriber(o).expect("Skip should not error");
    assert!(result.is_none());
}
