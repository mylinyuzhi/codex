use super::*;
use std::time::Duration;
use tempfile::tempdir;

#[tokio::test]
async fn test_cleanup_removes_non_active_with_zero_retention() {
    let dir = tempdir().expect("create temp dir");
    let snapshot_dir = dir.path();

    let old_snapshot = snapshot_dir.join("old-session.sh");
    let active_snapshot = snapshot_dir.join("active-session.sh");

    fs::write(&old_snapshot, "# old").await.expect("write old");
    fs::write(&active_snapshot, "# active")
        .await
        .expect("write active");

    // With zero retention, any non-active snapshot is immediately stale
    let removed = cleanup_stale_snapshots(snapshot_dir, "active-session", Duration::ZERO)
        .await
        .expect("cleanup");

    assert_eq!(removed, 1);
    assert!(!old_snapshot.exists());
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

    let invalid = snapshot_dir.join("no-extension");
    fs::write(&invalid, "# invalid").await.expect("write");

    let removed = cleanup_stale_snapshots(snapshot_dir, "other-session", Duration::from_secs(0))
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
