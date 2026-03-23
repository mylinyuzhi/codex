use super::*;

#[test]
fn test_is_process_alive_self() {
    let pid = std::process::id() as i32;
    assert!(is_process_alive(pid), "Current process should be alive");
}

#[test]
fn test_is_process_alive_invalid_pid() {
    assert!(!is_process_alive(0));
    assert!(!is_process_alive(-1));
}

#[test]
fn test_is_process_alive_nonexistent() {
    // PID 99999999 is very unlikely to exist
    assert!(!is_process_alive(99_999_999));
}

#[tokio::test]
async fn test_try_acquire_and_release() {
    let dir = tempfile::tempdir().expect("tempdir");
    let lock = InterProcessLock::try_acquire(dir.path(), "test-session-1")
        .await
        .expect("acquire");
    assert!(lock.is_some(), "Should acquire lock on fresh directory");

    let lock = lock.unwrap();
    assert!(lock.is_held());

    lock.release().await.expect("release");

    // Lock file should be gone
    assert!(!dir.path().join(LOCK_FILE).exists());
}

#[tokio::test]
async fn test_acquire_blocks_second_session() {
    let dir = tempfile::tempdir().expect("tempdir");

    let lock1 = InterProcessLock::try_acquire(dir.path(), "session-1")
        .await
        .expect("acquire 1");
    assert!(lock1.is_some());

    // Second session cannot acquire while first holds it
    let lock2 = InterProcessLock::try_acquire(dir.path(), "session-2")
        .await
        .expect("acquire 2");
    assert!(lock2.is_none(), "Second session should not acquire lock");

    // Release first
    lock1.unwrap().release().await.expect("release");

    // Now second session can acquire
    let lock2 = InterProcessLock::try_acquire(dir.path(), "session-2")
        .await
        .expect("acquire 2 after release");
    assert!(lock2.is_some());

    lock2.unwrap().release().await.expect("release 2");
}

#[tokio::test]
async fn test_same_session_reacquires() {
    let dir = tempfile::tempdir().expect("tempdir");

    let lock1 = InterProcessLock::try_acquire(dir.path(), "session-1")
        .await
        .expect("acquire");
    assert!(lock1.is_some());

    // Same session ID should reacquire (session restart scenario)
    let lock2 = InterProcessLock::try_acquire(dir.path(), "session-1")
        .await
        .expect("reacquire");
    assert!(lock2.is_some());

    // Clean up both (only one lock file exists)
    lock1.unwrap().release().await.expect("release 1");
    lock2.unwrap().release().await.expect("release 2");
}

#[tokio::test]
async fn test_lock_file_data_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("test.lock");
    let data = LockFileData {
        pid: 12345,
        session_id: "test".to_string(),
        acquired_at: 1711234567,
    };

    write_lock_file(&path, &data).await.expect("write");
    let loaded = read_lock_file(&path).await.expect("read");
    assert_eq!(loaded.pid, 12345);
    assert_eq!(loaded.session_id, "test");
}
