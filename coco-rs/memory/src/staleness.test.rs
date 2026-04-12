use super::*;
use std::time::SystemTime;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn test_memory_age_days_today() {
    let mtime = now_ms();
    assert_eq!(memory_age_days(mtime), 0);
}

#[test]
fn test_memory_age_days_yesterday() {
    let mtime = now_ms() - (24 * 60 * 60 * 1000);
    assert_eq!(memory_age_days(mtime), 1);
}

#[test]
fn test_memory_age_days_week_ago() {
    let mtime = now_ms() - (7 * 24 * 60 * 60 * 1000);
    assert_eq!(memory_age_days(mtime), 7);
}

#[test]
fn test_memory_age_text() {
    let now = now_ms();
    assert_eq!(memory_age(now), "today");
    assert_eq!(memory_age(now - 24 * 60 * 60 * 1000), "yesterday");
    // TS: returns "{N} days ago" for all values ≥2 (no weeks/months)
    assert_eq!(memory_age(now - 3 * 24 * 60 * 60 * 1000), "3 days ago");
    assert_eq!(memory_age(now - 10 * 24 * 60 * 60 * 1000), "10 days ago");
    assert_eq!(memory_age(now - 35 * 24 * 60 * 60 * 1000), "35 days ago");
}

#[test]
fn test_freshness_text_none_for_today() {
    let mtime = now_ms();
    assert!(memory_freshness_text(mtime).is_none());
}

#[test]
fn test_freshness_text_none_for_yesterday() {
    // TS: returns empty for ≤1 day (today AND yesterday)
    let mtime = now_ms() - (24 * 60 * 60 * 1000);
    assert!(memory_freshness_text(mtime).is_none());
}

#[test]
fn test_freshness_text_present_for_2_days() {
    let mtime = now_ms() - (2 * 24 * 60 * 60 * 1000);
    let text = memory_freshness_text(mtime);
    assert!(text.is_some());
    assert!(text.unwrap().contains("outdated"));
}

#[test]
fn test_freshness_note_has_system_reminder_tags() {
    let mtime = now_ms() - (5 * 24 * 60 * 60 * 1000);
    let note = memory_freshness_note(mtime).unwrap();
    assert!(note.starts_with("<system-reminder>"));
    assert!(note.ends_with("</system-reminder>"));
}

#[test]
fn test_staleness_info_fresh_today() {
    let info = StalenessInfo::from_mtime_ms(now_ms());
    assert_eq!(info.age_days, 0);
    assert!(!info.is_stale);
    assert!(info.warning.is_none());
}

#[test]
fn test_staleness_info_fresh_yesterday() {
    // Yesterday is NOT stale (≤1 day)
    let info = StalenessInfo::from_mtime_ms(now_ms() - 24 * 60 * 60 * 1000);
    assert_eq!(info.age_days, 1);
    assert!(!info.is_stale);
    assert!(info.warning.is_none());
}

#[test]
fn test_staleness_info_stale() {
    let mtime = now_ms() - (3 * 24 * 60 * 60 * 1000);
    let info = StalenessInfo::from_mtime_ms(mtime);
    assert_eq!(info.age_days, 3);
    assert_eq!(info.age_text, "3 days ago");
    assert!(info.is_stale);
    assert!(info.warning.is_some());
}
