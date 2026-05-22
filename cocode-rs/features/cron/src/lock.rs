//! Inter-process lock for multi-session coordination of durable tasks.
//!
//! Only one session at a time should run the scheduler for durable tasks.
//! The lock uses a file-based approach with PID liveness detection and
//! heartbeat renewal.

use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

/// Lock file name.
const LOCK_FILE: &str = "scheduled_tasks.lock";

/// Heartbeat interval in seconds.
const HEARTBEAT_INTERVAL_SECS: u64 = 30;

/// Lock retry interval in seconds.
const LOCK_RETRY_INTERVAL_SECS: u64 = 5;

/// Inter-process lock for coordinating durable cron task scheduling.
///
/// Only the session holding the lock runs durable tasks. Other sessions
/// should watch the task file for changes.
pub struct InterProcessLock {
    lock_path: PathBuf,
    heartbeat_cancel: CancellationToken,
}

/// Contents of the lock file.
#[derive(Debug, Serialize, Deserialize)]
struct LockFileData {
    pid: i32,
    session_id: String,
    acquired_at: i64,
}

impl InterProcessLock {
    /// Attempt to acquire the scheduler lock.
    ///
    /// Returns `Ok(Some(lock))` if acquired, `Ok(None)` if held by another
    /// live session, or `Err` on I/O failure.
    pub async fn try_acquire(
        cocode_home: &Path,
        session_id: &str,
    ) -> std::io::Result<Option<Self>> {
        let lock_path = cocode_home.join(LOCK_FILE);
        let pid = std::process::id() as i32;
        let now = now_unix_secs();
        let data = LockFileData {
            pid,
            session_id: session_id.to_string(),
            acquired_at: now,
        };

        // Step 1: Try atomic create
        if try_create_lock_file(&lock_path, &data).await {
            let lock = Self::new_with_heartbeat(lock_path, session_id.to_string());
            return Ok(Some(lock));
        }

        // Step 2: Read existing lock
        let existing = match read_lock_file(&lock_path).await {
            Some(existing) => existing,
            None => {
                // Lock file disappeared — retry create
                if try_create_lock_file(&lock_path, &data).await {
                    let lock = Self::new_with_heartbeat(lock_path, session_id.to_string());
                    return Ok(Some(lock));
                }
                return Ok(None);
            }
        };

        // Step 3: Check if we already own it (session restart)
        if existing.session_id == session_id {
            write_lock_file(&lock_path, &data).await?;
            let lock = Self::new_with_heartbeat(lock_path, session_id.to_string());
            return Ok(Some(lock));
        }

        // Step 4: Check if the owning process is still alive
        if is_process_alive(existing.pid) {
            tracing::debug!(
                existing_pid = existing.pid,
                "Scheduler lock held by active process"
            );
            return Ok(None);
        }

        // Step 5: Stale lock recovery
        tracing::info!(stale_pid = existing.pid, "Recovering stale scheduler lock");
        let _ = tokio::fs::remove_file(&lock_path).await;

        if try_create_lock_file(&lock_path, &data).await {
            let lock = Self::new_with_heartbeat(lock_path, session_id.to_string());
            return Ok(Some(lock));
        }

        Ok(None)
    }

    /// Retry acquiring the lock at regular intervals until cancelled.
    ///
    /// Returns the lock once acquired, or `None` if cancelled.
    pub async fn acquire_with_retry(
        cocode_home: &Path,
        session_id: &str,
        cancel: CancellationToken,
    ) -> std::io::Result<Option<Self>> {
        loop {
            match Self::try_acquire(cocode_home, session_id).await? {
                Some(lock) => return Ok(Some(lock)),
                None => {
                    tokio::select! {
                        _ = cancel.cancelled() => return Ok(None),
                        _ = tokio::time::sleep(
                            std::time::Duration::from_secs(LOCK_RETRY_INTERVAL_SECS)
                        ) => {}
                    }
                }
            }
        }
    }

    /// Release the lock by deleting the lock file and stopping the heartbeat.
    pub async fn release(self) -> std::io::Result<()> {
        self.heartbeat_cancel.cancel();
        let _ = tokio::fs::remove_file(&self.lock_path).await;
        tracing::debug!("Released scheduler lock");
        Ok(())
    }

    /// Whether this lock is still valid (heartbeat running).
    pub fn is_held(&self) -> bool {
        !self.heartbeat_cancel.is_cancelled()
    }

    fn new_with_heartbeat(lock_path: PathBuf, session_id: String) -> Self {
        let heartbeat_cancel = CancellationToken::new();
        let lock = Self {
            lock_path: lock_path.clone(),
            heartbeat_cancel: heartbeat_cancel.clone(),
        };

        // Start heartbeat task
        tokio::spawn(async move {
            let interval = std::time::Duration::from_secs(HEARTBEAT_INTERVAL_SECS);
            loop {
                tokio::select! {
                    _ = heartbeat_cancel.cancelled() => break,
                    _ = tokio::time::sleep(interval) => {}
                }

                let data = LockFileData {
                    pid: std::process::id() as i32,
                    session_id: session_id.clone(),
                    acquired_at: now_unix_secs(),
                };
                if let Err(e) = write_lock_file(&lock_path, &data).await {
                    tracing::warn!(error = %e, "Failed to update lock heartbeat");
                }
            }
        });

        lock
    }
}

impl Drop for InterProcessLock {
    fn drop(&mut self) {
        self.heartbeat_cancel.cancel();
    }
}

/// Try to atomically create the lock file. Returns `true` on success.
async fn try_create_lock_file(path: &Path, data: &LockFileData) -> bool {
    use tokio::io::AsyncWriteExt;

    let file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await;

    match file {
        Ok(mut f) => {
            let json = match serde_json::to_string(data) {
                Ok(j) => j,
                Err(_) => return false,
            };
            f.write_all(json.as_bytes()).await.is_ok()
        }
        Err(_) => false, // File already exists
    }
}

/// Write lock file contents (non-atomic, for heartbeat updates).
async fn write_lock_file(path: &Path, data: &LockFileData) -> std::io::Result<()> {
    let json = serde_json::to_string(data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    tokio::fs::write(path, json.as_bytes()).await
}

/// Read and parse the lock file.
async fn read_lock_file(path: &Path) -> Option<LockFileData> {
    let data = tokio::fs::read_to_string(path).await.ok()?;
    serde_json::from_str(&data).ok()
}

/// Check if a process is alive.
///
/// Uses `/proc/{pid}` on Linux (fast, no subprocess), with a `kill -0`
/// fallback for other Unix platforms (macOS, BSDs).
fn is_process_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    // Fast path: /proc/{pid} on Linux
    if std::fs::metadata(format!("/proc/{pid}")).is_ok() {
        return true;
    }
    // Fallback for macOS/BSDs: kill -0 checks existence without sending a signal
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_else(|e| {
            tracing::warn!("System clock before UNIX epoch: {e}");
            0
        })
}

#[cfg(test)]
#[path = "lock.test.rs"]
mod tests;
