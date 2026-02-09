use super::*;
use std::time::Duration;

#[test]
fn test_track_read() {
    let tracker = FileTracker::new();
    let state = ReadFileState::new("content".to_string(), None, 1);

    let is_trigger = tracker.track_read("/tmp/test.rs", state);
    assert!(!is_trigger);

    assert!(tracker.get_state("/tmp/test.rs").is_some());
}

#[test]
fn test_nested_memory_trigger() {
    let tracker = FileTracker::new();

    // Regular file - not a trigger
    let state = ReadFileState::new("content".to_string(), None, 1);
    assert!(!tracker.track_read("/project/src/main.rs", state));

    // CLAUDE.md - is a trigger
    let state = ReadFileState::new("instructions".to_string(), None, 1);
    assert!(tracker.track_read("/project/CLAUDE.md", state));

    // AGENTS.md - is a trigger
    let state = ReadFileState::new("agents".to_string(), None, 1);
    assert!(tracker.track_read("/project/AGENTS.md", state));

    assert!(tracker.has_nested_memory_triggers());

    let triggers = tracker.drain_nested_memory_triggers();
    assert_eq!(triggers.len(), 2);
    assert!(!tracker.has_nested_memory_triggers());
}

#[test]
fn test_partial_read() {
    let state = ReadFileState::partial("partial content".to_string(), None, 1, 100, 50);

    assert!(state.is_partial());

    let full = ReadFileState::new("full".to_string(), None, 1);
    assert!(!full.is_partial());
}

#[test]
fn test_tracked_files() {
    let tracker = FileTracker::new();

    tracker.track_read("/a.rs", ReadFileState::new("a".to_string(), None, 1));
    tracker.track_read("/b.rs", ReadFileState::new("b".to_string(), None, 1));
    tracker.track_read("/c.rs", ReadFileState::new("c".to_string(), None, 1));

    let files = tracker.tracked_files();
    assert_eq!(files.len(), 3);
}

#[test]
fn test_remove_tracking() {
    let tracker = FileTracker::new();

    tracker.track_read("/test.rs", ReadFileState::new("test".to_string(), None, 1));
    assert!(tracker.get_state("/test.rs").is_some());

    tracker.remove("/test.rs");
    assert!(tracker.get_state("/test.rs").is_none());
}

#[test]
fn test_clear() {
    let tracker = FileTracker::new();

    tracker.track_read("/a.rs", ReadFileState::new("a".to_string(), None, 1));
    tracker.track_read("/CLAUDE.md", ReadFileState::new("md".to_string(), None, 1));

    tracker.clear();

    assert!(tracker.tracked_files().is_empty());
    assert!(!tracker.has_nested_memory_triggers());
}

#[test]
fn test_mtime_comparison() {
    // Create a simple mtime
    let old_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
    let new_time = SystemTime::UNIX_EPOCH + Duration::from_secs(2000);

    let state = ReadFileState::new("content".to_string(), Some(old_time), 1);
    assert_eq!(state.last_modified, Some(old_time));

    // Newer time should indicate change
    assert!(new_time > old_time);
}