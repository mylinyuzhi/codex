//! KAIROS mode — daily logs and nightly consolidation.
//!
//! TS: services/autoDream/autoDream.ts — initAutoDream, consolidation triggers.
//! TS: services/autoDream/consolidationPrompt.ts — 4-phase consolidation.
//! TS: services/autoDream/consolidationLock.ts — lock mechanism.
//!
//! KAIROS (assistant mode) uses a different memory pattern:
//! - Daily: append-only log entries at `logs/YYYY/MM/YYYY-MM-DD.md`
//! - Nightly: consolidation agent distills logs → MEMORY.md + topic files

use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

/// Daily log directory structure within the memory directory.
///
/// Path pattern: `{memory_dir}/logs/{YYYY}/{MM}/{YYYY-MM-DD}.md`
pub fn daily_log_path(memory_dir: &Path, date: &str) -> PathBuf {
    // Parse date "YYYY-MM-DD"
    let parts: Vec<&str> = date.split('-').collect();
    let (year, month) = match parts.as_slice() {
        [y, m, ..] => (*y, *m),
        _ => ("unknown", "00"),
    };
    memory_dir
        .join("logs")
        .join(year)
        .join(month)
        .join(format!("{date}.md"))
}

/// Append an entry to the daily log.
///
/// Creates the log file and directory structure if they don't exist.
pub fn append_daily_log(
    memory_dir: &Path,
    date: &str,
    timestamp: &str,
    entry: &str,
) -> anyhow::Result<()> {
    let path = daily_log_path(memory_dir, date);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let line = format!("- [{timestamp}] {entry}\n");

    let mut content = std::fs::read_to_string(&path).unwrap_or_default();
    if content.is_empty() {
        content = format!("# Daily Log — {date}\n\n");
    }
    content.push_str(&line);
    std::fs::write(&path, content)?;

    Ok(())
}

/// Read the daily log for a given date.
pub fn read_daily_log(memory_dir: &Path, date: &str) -> Option<String> {
    let path = daily_log_path(memory_dir, date);
    std::fs::read_to_string(&path).ok()
}

// ── Consolidation lock ─────────────────────────────────────────────────

/// TS: consolidationLock.ts LOCK_FILE = '.consolidate-lock'
const LOCK_FILE: &str = ".consolidate-lock";

/// Stale lock threshold: 1 hour (TS: HOLDER_STALE_MS = 60 * 60 * 1000).
const HOLDER_STALE_SECS: u64 = 60 * 60;

/// Scan throttle: 10 minutes between session scans.
///
/// TS: SESSION_SCAN_INTERVAL_MS = 10 * 60 * 1000.
pub const SESSION_SCAN_INTERVAL_MS: i64 = 10 * 60 * 1000;

/// Lock state for consolidation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockState {
    /// Lock acquired, consolidation can proceed.
    Acquired,
    /// Lock held by another process.
    Held,
    /// Lock file could not be created.
    Error(String),
}

/// Try to acquire the consolidation lock.
///
/// TS: consolidationLock.ts tryAcquireConsolidationLock.
///
/// Returns `Acquired(prior_mtime_ms)` with the prior lock mtime (0 if new).
/// Returns `Held` if another process holds a fresh lock.
/// The lock file body contains the holder's PID.
/// Mtime of the lock file serves as `lastConsolidatedAt`.
pub fn try_acquire_lock(memory_dir: &Path) -> LockState {
    let lock_path = memory_dir.join(LOCK_FILE);

    if lock_path.exists() {
        // Read existing lock: check PID + staleness
        let holder_pid = std::fs::read_to_string(&lock_path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok());

        if let Ok(metadata) = std::fs::metadata(&lock_path)
            && let Ok(modified) = metadata.modified()
        {
            let age = SystemTime::now()
                .duration_since(modified)
                .unwrap_or_default();

            if age.as_secs() < HOLDER_STALE_SECS {
                // Lock is fresh — check if holder PID is alive
                if holder_pid.is_some_and(is_process_running) {
                    return LockState::Held;
                }
                // Dead PID: reclaim
            }
            // Stale lock or dead PID: reclaim by falling through
        }

        let _ = std::fs::remove_file(&lock_path);

        if lock_path.exists() {
            return LockState::Held;
        }
    }

    // Write our PID
    let our_pid = std::process::id();
    match std::fs::write(&lock_path, our_pid.to_string()) {
        Ok(()) => {
            // Verify we won the race (re-read and check PID)
            let read_back = std::fs::read_to_string(&lock_path)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok());
            if read_back == Some(our_pid) {
                LockState::Acquired
            } else {
                LockState::Held
            }
        }
        Err(e) => LockState::Error(e.to_string()),
    }
}

/// Check if a process is still running (Unix: kill(pid, 0) probe).
fn is_process_running(pid: u32) -> bool {
    if pid <= 1 {
        return false;
    }
    // Signal 0 checks existence without sending a real signal
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        // Conservative: assume alive on non-Unix platforms
        true
    }
}

/// Release the consolidation lock.
pub fn release_lock(memory_dir: &Path) {
    let lock_path = memory_dir.join(LOCK_FILE);
    let _ = std::fs::remove_file(&lock_path);
}

/// Rollback the consolidation lock to a prior mtime.
///
/// TS: consolidationLock.ts rollbackConsolidationLock.
/// On failure, restores the lock mtime so the time-gate resets correctly.
pub fn rollback_lock(memory_dir: &Path, prior_mtime_ms: i64) {
    let lock_path = memory_dir.join(LOCK_FILE);
    if prior_mtime_ms == 0 {
        // Lock didn't exist before — just remove it
        let _ = std::fs::remove_file(&lock_path);
    } else {
        // Restore the prior mtime by rewriting with empty body and setting mtime
        let _ = std::fs::write(&lock_path, "");
        let secs = prior_mtime_ms / 1000;
        let time = filetime::FileTime::from_unix_time(secs, 0);
        let _ = filetime::set_file_mtime(&lock_path, time);
    }
}

/// Read the timestamp of the last successful consolidation.
///
/// TS: consolidationLock.ts readLastConsolidatedAt — uses lock file mtime.
pub fn read_last_consolidated_at(memory_dir: &Path) -> Option<i64> {
    let lock_path = memory_dir.join(LOCK_FILE);
    crate::staleness::file_mtime_ms(&lock_path)
}

/// Record a successful consolidation by writing the lock file.
///
/// TS: consolidationLock.ts recordConsolidation.
pub fn record_consolidation(memory_dir: &Path) -> anyhow::Result<()> {
    if let Some(parent) = memory_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(memory_dir)?;
    let lock_path = memory_dir.join(LOCK_FILE);
    std::fs::write(&lock_path, std::process::id().to_string())?;
    Ok(())
}

// ── Consolidation gates ────────────────────────────────────────────────

/// Check if auto-dream consolidation should run.
///
/// Gates:
/// 1. Time gate: at least `min_hours` since last consolidation
/// 2. Session gate: at least `min_sessions` sessions since last
pub fn should_consolidate(
    memory_dir: &Path,
    min_hours: i32,
    min_sessions: i32,
    session_count_since_last: i32,
) -> bool {
    // Time gate
    let last = read_last_consolidated_at(memory_dir);
    if let Some(last_ms) = last {
        let now_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let hours_since = (now_ms - last_ms) / (60 * 60 * 1000);
        if hours_since < min_hours as i64 {
            return false;
        }
    }
    // If no last consolidation, time gate passes (first time)

    // Session gate
    session_count_since_last >= min_sessions
}

// ── Consolidation prompt ───────────────────────────────────────────────

/// Build the 4-phase consolidation prompt for the auto-dream agent.
///
/// TS: autoDream/consolidationPrompt.ts — buildConsolidationPrompt.
pub fn build_consolidation_prompt(
    memory_dir: &Path,
    transcript_dir: &Path,
    sessions_since_last: &[String],
) -> String {
    let session_list = if sessions_since_last.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nSessions since last consolidation ({}):\n{}",
            sessions_since_last.len(),
            sessions_since_last
                .iter()
                .map(|s| format!("- {s}"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    };

    format!(
        "# Dream: Memory Consolidation\n\n\
         Memory directory: `{mem}` — this directory already exists.\n\
         Transcript directory: `{trans}` — contains large JSONL files, grep narrowly.\n\n\
         Tools available: Read, Grep, Glob (unrestricted), Bash (read-only: ls, find, grep, \
         cat, stat, wc, head, tail), Edit/Write (memory directory only).\n\n\
         ## Phase 1: Orient\n\
         - `ls` the memory directory\n\
         - Read MEMORY.md (the current index)\n\
         - Skim existing topic files\n\
         - Review logs/sessions/ subdirs if present\n\n\
         ## Phase 2: Gather Recent Signal\n\
         Priority order:\n\
         1. Daily logs (logs/YYYY/MM/YYYY-MM-DD.md)\n\
         2. Existing memories that drifted (especially project memories)\n\
         3. Transcript search (narrow grep on JSONL, `tail -50`)\n\n\
         ## Phase 3: Consolidate\n\
         - Write/update memory files using YAML frontmatter format\n\
         - Merge new signal into existing files rather than duplicating\n\
         - Convert relative dates → absolute dates\n\
         - Delete contradicted facts\n\n\
         ## Phase 4: Prune & Index\n\
         - Update MEMORY.md (keep under 200 lines / ~25KB)\n\
         - Index format: `- [Title](file.md) — one-line hook` (each <150 chars)\n\
         - Remove stale/wrong/superseded pointers\n\
         - Demote verbose entries (>200 chars = content belongs in topic file)\n\
         - Resolve contradictions\n\
         - Ensure no sensitive data in team/ memories{session_list}",
        mem = memory_dir.display(),
        trans = transcript_dir.display(),
    )
}

/// List daily log files modified since a given timestamp.
pub fn list_logs_since(memory_dir: &Path, since_ms: i64) -> Vec<PathBuf> {
    let logs_dir = memory_dir.join("logs");
    if !logs_dir.is_dir() {
        return Vec::new();
    }

    let mut logs = Vec::new();
    collect_log_files(&logs_dir, since_ms, &mut logs);
    logs.sort();
    logs
}

fn collect_log_files(dir: &Path, since_ms: i64, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_log_files(&path, since_ms, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            let mtime = crate::staleness::file_mtime_ms(&path).unwrap_or(0);
            if mtime >= since_ms {
                out.push(path);
            }
        }
    }
}

#[cfg(test)]
#[path = "kairos.test.rs"]
mod tests;
