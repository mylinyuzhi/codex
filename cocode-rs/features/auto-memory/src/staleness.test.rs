use std::time::Duration;

use super::*;

/// Default staleness threshold matching the protocol constant.
const DEFAULT_THRESHOLD: i64 = 1;

#[test]
fn test_format_relative_time_today() {
    assert_eq!(format_relative_time(0), "today");
}

#[test]
fn test_format_relative_time_yesterday() {
    assert_eq!(format_relative_time(1), "yesterday");
}

#[test]
fn test_format_relative_time_days_ago() {
    assert_eq!(format_relative_time(5), "5 days ago");
}

#[test]
fn test_format_relative_time_many_days() {
    assert_eq!(format_relative_time(365), "365 days ago");
}

#[test]
fn test_staleness_warning_fresh() {
    assert!(build_staleness_warning(0, DEFAULT_THRESHOLD).is_empty());
    assert!(build_staleness_warning(1, DEFAULT_THRESHOLD).is_empty());
}

#[test]
fn test_staleness_warning_boundary() {
    // Exactly 2 days — should warn with default threshold of 1
    let warning = build_staleness_warning(2, DEFAULT_THRESHOLD);
    assert!(warning.contains("2 days old"));
}

#[test]
fn test_staleness_warning_stale() {
    let warning = build_staleness_warning(3, DEFAULT_THRESHOLD);
    assert!(warning.contains("3 days old"));
    assert!(warning.contains("point-in-time"));
}

#[test]
fn test_staleness_warning_very_old() {
    let warning = build_staleness_warning(365, DEFAULT_THRESHOLD);
    assert!(warning.contains("365 days old"));
}

#[test]
fn test_staleness_warning_custom_threshold() {
    // With threshold of 7 days, 5-day-old memory is still fresh
    assert!(build_staleness_warning(5, 7).is_empty());
    // But 8-day-old memory should warn
    assert!(!build_staleness_warning(8, 7).is_empty());
}

#[test]
fn test_staleness_info_now() {
    let info = staleness_info(SystemTime::now(), DEFAULT_THRESHOLD);
    assert_eq!(info.days_since_modified, 0);
    assert_eq!(info.relative_time, "today");
    assert!(!info.needs_warning);
    assert!(info.warning.is_empty());
}

#[test]
fn test_staleness_info_future_timestamp() {
    // Future timestamp should return 0 days (not negative)
    let future = SystemTime::now() + Duration::from_secs(86400);
    let info = staleness_info(future, DEFAULT_THRESHOLD);
    assert_eq!(info.days_since_modified, 0);
    assert_eq!(info.relative_time, "today");
    assert!(!info.needs_warning);
}

#[test]
fn test_staleness_info_old() {
    let old = SystemTime::now() - Duration::from_secs(86400 * 5);
    let info = staleness_info(old, DEFAULT_THRESHOLD);
    assert_eq!(info.days_since_modified, 5);
    assert_eq!(info.relative_time, "5 days ago");
    assert!(info.needs_warning);
    assert!(info.warning.contains("5 days old"));
}
