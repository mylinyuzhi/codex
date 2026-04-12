use super::*;

#[test]
fn test_daily_log_path() {
    let mem_dir = std::path::Path::new("/home/.claude/memory");
    let path = daily_log_path(mem_dir, "2026-04-06");
    assert_eq!(
        path,
        std::path::PathBuf::from("/home/.claude/memory/logs/2026/04/2026-04-06.md"),
    );
}

#[test]
fn test_append_daily_log() {
    let dir = tempfile::tempdir().unwrap();
    append_daily_log(
        dir.path(),
        "2026-04-06",
        "14:30",
        "User prefers short responses",
    )
    .unwrap();

    let content = read_daily_log(dir.path(), "2026-04-06").unwrap();
    assert!(content.contains("# Daily Log — 2026-04-06"));
    assert!(content.contains("[14:30] User prefers short responses"));

    // Append a second entry
    append_daily_log(dir.path(), "2026-04-06", "15:00", "Discovered test pattern").unwrap();
    let content = read_daily_log(dir.path(), "2026-04-06").unwrap();
    assert!(content.contains("[14:30]"));
    assert!(content.contains("[15:00]"));
}

#[test]
fn test_lock_file_name() {
    // TS uses '.consolidate-lock' (with hyphen)
    assert_eq!(LOCK_FILE, ".consolidate-lock");
}

#[test]
fn test_consolidation_lock() {
    let dir = tempfile::tempdir().unwrap();

    // First acquire should succeed
    assert_eq!(try_acquire_lock(dir.path()), LockState::Acquired);

    // Second acquire should fail (held by our own PID which is running)
    assert_eq!(try_acquire_lock(dir.path()), LockState::Held);

    // Release
    release_lock(dir.path());

    // Should be acquirable again
    assert_eq!(try_acquire_lock(dir.path()), LockState::Acquired);
    release_lock(dir.path());
}

#[test]
fn test_last_consolidated_at_from_lock_mtime() {
    let dir = tempfile::tempdir().unwrap();

    // No lock file → None
    assert!(read_last_consolidated_at(dir.path()).is_none());

    // After recording consolidation → Some(mtime)
    record_consolidation(dir.path()).unwrap();
    let ts = read_last_consolidated_at(dir.path()).unwrap();
    assert!(ts > 0);
}

#[test]
fn test_rollback_lock_removes_when_prior_zero() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join(LOCK_FILE);

    // Create a lock file
    std::fs::write(&lock_path, "12345").unwrap();
    assert!(lock_path.exists());

    // Rollback with prior_mtime=0 should remove
    rollback_lock(dir.path(), 0);
    assert!(!lock_path.exists());
}

#[test]
fn test_should_consolidate_first_time() {
    let dir = tempfile::tempdir().unwrap();
    // No last consolidation → time gate passes, need session gate
    assert!(should_consolidate(dir.path(), 24, 5, 5));
    assert!(!should_consolidate(dir.path(), 24, 5, 2));
}

#[test]
fn test_should_consolidate_too_soon() {
    let dir = tempfile::tempdir().unwrap();
    record_consolidation(dir.path()).unwrap();
    // Just consolidated → time gate fails
    assert!(!should_consolidate(dir.path(), 24, 5, 10));
}

#[test]
fn test_list_logs_since() {
    let dir = tempfile::tempdir().unwrap();
    append_daily_log(dir.path(), "2026-04-06", "10:00", "entry").unwrap();

    let logs = list_logs_since(dir.path(), 0);
    assert_eq!(logs.len(), 1);

    // Future timestamp should find nothing
    let future = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
        + 100_000_000;
    let logs = list_logs_since(dir.path(), future);
    assert!(logs.is_empty());
}

#[test]
fn test_consolidation_prompt() {
    let mem_dir = std::path::Path::new("/home/.claude/memory");
    let trans_dir = std::path::Path::new("/home/.claude/projects/test/transcripts");
    let sessions = vec!["session-1".to_string(), "session-2".to_string()];
    let prompt = build_consolidation_prompt(mem_dir, trans_dir, &sessions);
    assert!(prompt.contains("Phase 1"));
    assert!(prompt.contains("Phase 2"));
    assert!(prompt.contains("Phase 3"));
    assert!(prompt.contains("Phase 4"));
    assert!(prompt.contains("200 lines"));
    assert!(prompt.contains("transcripts"));
    assert!(prompt.contains("session-1"));
    assert!(prompt.contains("2)"));
}

#[test]
fn test_session_scan_interval() {
    // TS: SESSION_SCAN_INTERVAL_MS = 10 * 60 * 1000
    assert_eq!(SESSION_SCAN_INTERVAL_MS, 600_000);
}
