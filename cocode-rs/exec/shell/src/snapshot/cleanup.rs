//! Stale snapshot cleanup utilities.
//!
//! Provides functions to clean up orphaned or expired shell snapshot files.

use std::io::ErrorKind;
use std::path::Path;
use std::time::Duration;
use std::time::SystemTime;

use anyhow::Result;
use tokio::fs;

/// Removes stale shell snapshot files from the snapshot directory.
///
/// A snapshot is considered stale if:
/// 1. It lacks a valid session ID format (no extension separator)
/// 2. It belongs to a session other than the active one and is older than the retention period
///
/// # Arguments
///
/// * `snapshot_dir` - Directory containing snapshot files
/// * `active_session_id` - Session ID to exempt from cleanup (currently active)
/// * `retention` - How long to keep inactive snapshots before removal
///
/// # Returns
///
/// Returns the number of snapshots removed, or an error if cleanup fails.
pub async fn cleanup_stale_snapshots(
    snapshot_dir: &Path,
    active_session_id: &str,
    retention: Duration,
) -> Result<i32> {
    let mut entries = match fs::read_dir(snapshot_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err.into()),
    };

    let now = SystemTime::now();
    let mut removed_count = 0;

    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await?.is_file() {
            continue;
        }

        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        // Extract session ID from filename (format: {session_id}.{extension})
        let session_id = match file_name.rsplit_once('.') {
            Some((stem, _ext)) => stem,
            None => {
                // Invalid filename format, remove it
                remove_snapshot_file(&path).await;
                removed_count += 1;
                continue;
            }
        };

        // Don't remove the active session's snapshot
        if session_id == active_session_id {
            continue;
        }

        // Check if the snapshot is older than the retention period
        let modified = match fs::metadata(&path).await.and_then(|m| m.modified()) {
            Ok(modified) => modified,
            Err(err) => {
                tracing::warn!(
                    "Failed to check snapshot age for {}: {err:?}",
                    path.display()
                );
                continue;
            }
        };

        if let Ok(age) = now.duration_since(modified) {
            if age >= retention {
                remove_snapshot_file(&path).await;
                removed_count += 1;
            }
        }
    }

    Ok(removed_count)
}

/// Removes all snapshot files for a specific session.
///
/// This is useful for cleaning up after a session ends.
#[allow(dead_code)]
pub async fn cleanup_session_snapshots(snapshot_dir: &Path, session_id: &str) -> Result<()> {
    let mut entries = match fs::read_dir(snapshot_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };

    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await?.is_file() {
            continue;
        }

        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        // Check if this file belongs to the target session
        if let Some((stem, _ext)) = file_name.rsplit_once('.') {
            if stem == session_id {
                remove_snapshot_file(&entry.path()).await;
            }
        }
    }

    Ok(())
}

/// Removes a snapshot file, logging any errors.
async fn remove_snapshot_file(path: &Path) {
    if let Err(err) = fs::remove_file(path).await {
        if err.kind() != ErrorKind::NotFound {
            tracing::warn!("Failed to delete shell snapshot at {:?}: {err:?}", path);
        }
    } else {
        tracing::debug!("Removed stale snapshot: {}", path.display());
    }
}

#[cfg(test)]
#[path = "cleanup.test.rs"]
mod tests;
