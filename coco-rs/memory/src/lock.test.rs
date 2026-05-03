use super::*;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[test]
fn first_acquire_returns_acquired_with_zero_prior() {
    let temp = tempdir().unwrap();
    match try_acquire(temp.path()) {
        LockOutcome::Acquired { prior_mtime_ms } => assert_eq!(prior_mtime_ms, 0),
        other => panic!("expected Acquired, got {other:?}"),
    }
}

#[test]
fn second_acquire_returns_held_when_first_alive() {
    let temp = tempdir().unwrap();
    let _first = try_acquire(temp.path());
    // Same process, lock body is our PID, mtime is fresh — should be Held.
    assert_eq!(try_acquire(temp.path()), LockOutcome::Held);
}

#[test]
fn release_clears_the_lock() {
    let temp = tempdir().unwrap();
    let _ = try_acquire(temp.path());
    release(temp.path());
    assert!(!temp.path().join(LOCK_FILENAME).exists());
}

#[test]
fn rollback_with_zero_removes_lock() {
    let temp = tempdir().unwrap();
    let _ = try_acquire(temp.path());
    rollback(temp.path(), 0);
    assert!(!temp.path().join(LOCK_FILENAME).exists());
}

#[test]
fn rollback_with_prior_restores_mtime() {
    let temp = tempdir().unwrap();
    let _ = try_acquire(temp.path());
    rollback(temp.path(), 1_700_000_000_000);
    let mtime = last_consolidated_at(temp.path()).unwrap();
    // Allow rounding error from filetime sub-second precision.
    assert!((mtime - 1_700_000_000_000).abs() < 2_000);
}
