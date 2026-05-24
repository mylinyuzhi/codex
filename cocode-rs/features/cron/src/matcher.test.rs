use chrono::TimeZone;

use super::*;

fn make_time(hour: u32, minute: u32) -> chrono::DateTime<chrono::Utc> {
    // 2026-03-23 is a Monday (weekday = 1 in cron)
    chrono::Utc
        .with_ymd_and_hms(2026, 3, 23, hour, minute, 0)
        .unwrap()
}

#[test]
fn test_wildcard_matches_everything() {
    let t = make_time(14, 30);
    assert!(matches_cron("* * * * *", &t));
}

#[test]
fn test_exact_minute_match() {
    let t = make_time(14, 30);
    assert!(matches_cron("30 * * * *", &t));
    assert!(!matches_cron("31 * * * *", &t));
}

#[test]
fn test_exact_hour_match() {
    let t = make_time(14, 30);
    assert!(matches_cron("30 14 * * *", &t));
    assert!(!matches_cron("30 15 * * *", &t));
}

#[test]
fn test_step_pattern() {
    let t = make_time(14, 30);
    assert!(matches_cron("*/5 * * * *", &t)); // 30 is divisible by 5
    assert!(matches_cron("*/10 * * * *", &t)); // 30 is divisible by 10
    assert!(!matches_cron("*/7 * * * *", &t)); // 30 is not divisible by 7
}

#[test]
fn test_range_match() {
    let t = make_time(14, 30);
    assert!(matches_cron("25-35 * * * *", &t));
    assert!(!matches_cron("0-29 * * * *", &t));
}

#[test]
fn test_range_with_step() {
    let t = make_time(14, 30);
    assert!(matches_cron("0-30/10 * * * *", &t)); // 30 - 0 = 30, 30 % 10 = 0
    assert!(!matches_cron("0-30/7 * * * *", &t)); // 30 % 7 != 0
}

#[test]
fn test_comma_separated() {
    let t = make_time(14, 30);
    assert!(matches_cron("0,15,30,45 * * * *", &t));
    assert!(!matches_cron("0,15,45 * * * *", &t));
}

#[test]
fn test_day_of_week() {
    // 2026-03-23 is a Monday (cron weekday = 1)
    let t = make_time(9, 0);
    assert!(matches_cron("0 9 * * 1", &t)); // Monday
    assert!(!matches_cron("0 9 * * 0", &t)); // Sunday
    assert!(matches_cron("0 9 * * 1-5", &t)); // Weekdays
}

#[test]
fn test_month_match() {
    // March = 3
    let t = make_time(9, 0);
    assert!(matches_cron("0 9 * 3 *", &t));
    assert!(!matches_cron("0 9 * 4 *", &t));
}

#[test]
fn test_day_of_month() {
    // Day = 23
    let t = make_time(9, 0);
    assert!(matches_cron("0 9 23 * *", &t));
    assert!(!matches_cron("0 9 24 * *", &t));
}

#[test]
fn test_invalid_cron_returns_false() {
    let t = make_time(9, 0);
    assert!(!matches_cron("* * *", &t)); // only 3 fields
    assert!(!matches_cron("", &t));
}

#[test]
fn test_question_mark_wildcard() {
    let t = make_time(14, 30);
    assert!(matches_cron("? ? ? ? ?", &t));
}

#[test]
fn test_exact_with_step() {
    let t = make_time(14, 15);
    assert!(matches_cron("5/10 * * * *", &t)); // 15 >= 5 and (15-5) % 10 == 0
    assert!(!matches_cron("5/10 * * * *", &make_time(14, 12))); // (12-5) % 10 != 0
}
