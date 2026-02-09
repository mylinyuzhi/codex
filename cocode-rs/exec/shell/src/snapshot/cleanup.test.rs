use super::*;
use std::time::Duration;
use tempfile::tempdir;

#[tokio::test]
async fn test_cleanup_removes_old_snapshots() {
    let dir = tempdir().expect("create temp dir");
    let snapshot_dir = dir.path();

    // Create some snapshot files
    let old_snapshot = snapshot_dir.join("old-session.sh");
    let active_snapshot = snapshot_dir.join("active-session.sh");

    fs::write(&old_snapshot, "# old").await.expect("write old");
    fs::write(&active_snapshot, "# active")
        .await
        .expect("write active");

    // Set old snapshot's mtime to the past
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let old_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time")
            .as_secs()
            - 60 * 60 * 24 * 8; // 8 days ago

        let ts = libc::timespec {
            tv_sec: old_time as libc::time_t,
            tv_nsec: 0,
        };
        let times = [ts, ts];
        let c_path =
            std::ffi::CString::new(old_snapshot.as_os_str().as_bytes()).expect("cstring");
        unsafe {
            libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), times.as_ptr(), 0);
        }
    }

    // Run cleanup with 7-day retention
    let removed = cleanup_stale_snapshots(
        snapshot_dir,
        "active-session",
        Duration::from_secs(60 * 60 * 24 * 7),
    )
    .await
    .expect("cleanup");

    // On Unix, the old snapshot should have been removed
    #[cfg(unix)]
    {
        assert_eq!(removed, 1);
        assert!(!old_snapshot.exists());
    }

    // Active snapshot should still exist
    assert!(active_snapshot.exists());
}

#[tokio::test]
async fn test_cleanup_skips_active_session() {
    let dir = tempdir().expect("create temp dir");
    let snapshot_dir = dir.path();

    let active_snapshot = snapshot_dir.join("my-session.sh");
    fs::write(&active_snapshot, "# active")
        .await
        .expect("write");

    // Cleanup should skip the active session even with zero retention
    let removed = cleanup_stale_snapshots(snapshot_dir, "my-session", Duration::from_secs(0))
        .await
        .expect("cleanup");

    assert_eq!(removed, 0);
    assert!(active_snapshot.exists());
}

#[tokio::test]
async fn test_cleanup_removes_invalid_filenames() {
    let dir = tempdir().expect("create temp dir");
    let snapshot_dir = dir.path();

    // Create a file without extension
    let invalid = snapshot_dir.join("no-extension");
    fs::write(&invalid, "# invalid").await.expect("write");

    let removed =
        cleanup_stale_snapshots(snapshot_dir, "other-session", Duration::from_secs(0))
            .await
            .expect("cleanup");

    assert_eq!(removed, 1);
    assert!(!invalid.exists());
}

#[tokio::test]
async fn test_cleanup_handles_missing_dir() {
    let dir = tempdir().expect("create temp dir");
    let nonexistent = dir.path().join("nonexistent");

    let removed = cleanup_stale_snapshots(&nonexistent, "session", Duration::from_secs(0))
        .await
        .expect("cleanup");

    assert_eq!(removed, 0);
}

#[tokio::test]
async fn test_cleanup_session_snapshots() {
    let dir = tempdir().expect("create temp dir");
    let snapshot_dir = dir.path();

    let target_sh = snapshot_dir.join("target-session.sh");
    let target_ps1 = snapshot_dir.join("target-session.ps1");
    let other = snapshot_dir.join("other-session.sh");

    fs::write(&target_sh, "# target").await.expect("write");
    fs::write(&target_ps1, "# target ps1").await.expect("write");
    fs::write(&other, "# other").await.expect("write");

    cleanup_session_snapshots(snapshot_dir, "target-session")
        .await
        .expect("cleanup");

    assert!(!target_sh.exists());
    assert!(!target_ps1.exists());
    assert!(other.exists());
}
