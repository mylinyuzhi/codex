//! PID + mtime CAS lock for auto-dream consolidation.
//!
//! TS: `services/autoDream/consolidationLock.ts`. The lock file's
//! **mtime** is the `lastConsolidatedAt` timestamp; its body is the
//! holder's PID. A stale-PID + 1h-mtime threshold lets a crashed parent
//! release the lock for a follow-up reclaim.

use std::path::Path;
use std::time::SystemTime;

/// Lock file basename inside the memory directory.
pub const LOCK_FILENAME: &str = ".consolidate-lock";

/// Dead-PID reclaim threshold. A lock older than this is reclaimable
/// even if the holder PID is alive (defensive: prevents stuck locks).
pub const HOLDER_STALE_SECS: u64 = 60 * 60;

/// Lock acquisition outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockOutcome {
    /// Lock acquired. Carries the prior mtime in milliseconds (0 = lock
    /// didn't exist before). Hand this to [`rollback`] on consolidation
    /// failure so the time-gate doesn't reset to "now."
    Acquired { prior_mtime_ms: i64 },
    /// Lock is held by a live PID with a fresh mtime.
    Held,
    /// Filesystem error during acquisition.
    Error(String),
}

/// Try to acquire the consolidation lock at `<memory_dir>/.consolidate-lock`.
pub fn try_acquire(memory_dir: &Path) -> LockOutcome {
    let lock_path = memory_dir.join(LOCK_FILENAME);

    let prior_mtime_ms = read_mtime_ms(&lock_path).unwrap_or(0);

    if lock_path.exists() {
        let holder_pid = std::fs::read_to_string(&lock_path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok());
        let age_secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs())
            .map(|now| now.saturating_sub((prior_mtime_ms / 1000) as u64))
            .unwrap_or(u64::MAX);

        let fresh = age_secs < HOLDER_STALE_SECS;
        let alive = holder_pid.is_some_and(is_process_running);

        if fresh && alive {
            return LockOutcome::Held;
        }
        // Stale or dead — fall through and reclaim. NB: do NOT
        // `remove_file` here. TS parity (`consolidationLock.ts:71-81`)
        // uses a single `writeFile` (POSIX `O_TRUNC | O_CREAT`) which
        // atomically overwrites without an unlink window — two
        // reclaimers racing both write, the read-back-and-verify below
        // picks one winner. A `remove_file` step would open a TOCTOU
        // gap where both reclaimers pass the staleness check, both
        // unlink (one ENOENT-tolerated), both write, and the loser
        // silently overwrites the winner before its read-back fires.
    }

    if let Some(parent) = lock_path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return LockOutcome::Error(format!("could not create memory dir: {e}"));
    }
    let our_pid = std::process::id();
    if let Err(e) = std::fs::write(&lock_path, our_pid.to_string()) {
        return LockOutcome::Error(format!("could not write lock: {e}"));
    }

    // CAS verify: the file we just wrote should still hold our PID.
    // Two reclaimers might race; the loser sees its PID overwritten.
    let read_back = std::fs::read_to_string(&lock_path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());
    if read_back == Some(our_pid) {
        LockOutcome::Acquired { prior_mtime_ms }
    } else {
        LockOutcome::Held
    }
}

/// Release the lock unconditionally — call on successful consolidation
/// completion. Idempotent.
pub fn release(memory_dir: &Path) {
    let _ = std::fs::remove_file(memory_dir.join(LOCK_FILENAME));
}

/// Roll the lock mtime back to `prior_mtime_ms` after a failed run, so
/// the time-gate resets to the previous successful consolidation
/// rather than "now." If `prior_mtime_ms == 0`, the lock didn't exist
/// before — remove it.
pub fn rollback(memory_dir: &Path, prior_mtime_ms: i64) {
    let lock_path = memory_dir.join(LOCK_FILENAME);
    if prior_mtime_ms == 0 {
        let _ = std::fs::remove_file(&lock_path);
        return;
    }
    if std::fs::write(&lock_path, "").is_err() {
        return;
    }
    let secs = prior_mtime_ms / 1000;
    let nanos = ((prior_mtime_ms % 1000) * 1_000_000) as u32;
    let time = filetime::FileTime::from_unix_time(secs, nanos);
    let _ = filetime::set_file_mtime(&lock_path, time);
}

/// Last successful consolidation timestamp (lock file mtime in ms),
/// or `None` if no lock has ever been written.
pub fn last_consolidated_at(memory_dir: &Path) -> Option<i64> {
    read_mtime_ms(&memory_dir.join(LOCK_FILENAME))
}

/// Stamp a successful consolidation — used when /dream runs manually
/// outside the normal lock cycle.
pub fn record_consolidation(memory_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(memory_dir)?;
    std::fs::write(
        memory_dir.join(LOCK_FILENAME),
        std::process::id().to_string(),
    )
}

fn read_mtime_ms(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

fn is_process_running(pid: u32) -> bool {
    if pid <= 1 {
        return false;
    }
    #[cfg(unix)]
    {
        // Signal 0 probes existence without delivering a signal.
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        // Conservative on non-Unix.
        true
    }
}

#[cfg(test)]
#[path = "lock.test.rs"]
mod tests;
