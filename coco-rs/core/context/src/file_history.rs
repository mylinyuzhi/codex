//! File edit tracking with per-turn snapshots and content-addressed backups.
//!
//! TS: fileHistory.ts (~1110 LOC) — content-addressed backups, rewind, session resume.
//!
//! Three-phase async pattern (from TS):
//! 1. Capture state (read current)
//! 2. Async I/O (backup files outside state lock)
//! 3. Commit (update state with fresh read)

use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use tokio::fs;

/// Maximum snapshots retained per session (oldest evicted).
const MAX_SNAPSHOTS: usize = 100;

/// Tracks file edits across the conversation for undo/snapshot capability.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileHistoryState {
    /// Ordered snapshots (newest last, max `MAX_SNAPSHOTS`).
    pub snapshots: Vec<FileHistorySnapshot>,
    /// Files currently being tracked.
    pub tracked_files: HashSet<PathBuf>,
    /// Monotonically increasing counter (activity signal).
    pub snapshot_sequence: i64,
}

/// A snapshot of file states at a particular point in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHistorySnapshot {
    /// Message UUID that triggered this snapshot.
    pub message_id: String,
    /// Per-file backup info.
    pub tracked_file_backups: HashMap<PathBuf, FileHistoryBackup>,
    /// When the snapshot was taken (epoch ms).
    pub timestamp: i64,
}

/// Backup info for a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHistoryBackup {
    /// Content-addressed backup name. `None` = file didn't exist at this version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_file_name: Option<String>,
    /// Version counter for this file within the session.
    pub version: i32,
    /// When the backup was created (epoch ms).
    pub backup_time: i64,
}

/// Result of a diff stats preview (what `rewind` would change).
#[derive(Debug, Clone, Default)]
pub struct DiffStats {
    pub files_changed: Vec<PathBuf>,
    pub insertions: i64,
    pub deletions: i64,
}

/// Resolves the backup directory for a session.
pub fn backup_dir(config_home: &Path, session_id: &str) -> PathBuf {
    config_home.join("file-history").join(session_id)
}

/// Content-addressed backup file name: first 16 hex chars of SHA-256(path) + version.
fn backup_file_name(file_path: &Path, version: i32) -> String {
    let hash = Sha256::digest(file_path.as_os_str().as_encoded_bytes());
    let hex = hex_encode_16(&hash);
    format!("{hex}@v{version}")
}

/// First 16 hex chars of a hash digest.
fn hex_encode_16(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(8)
        .fold(String::with_capacity(16), |mut s, b| {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Resolve full path to a backup file on disk.
fn resolve_backup_path(config_home: &Path, session_id: &str, backup_name: &str) -> PathBuf {
    backup_dir(config_home, session_id).join(backup_name)
}

/// Check if the origin file has changed compared to a backup (stat-based fast path).
async fn origin_file_changed(origin: &Path, backup_path: &Path) -> Result<bool> {
    let origin_meta = match fs::metadata(origin).await {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(e) => return Err(e.into()),
    };
    let backup_meta = match fs::metadata(backup_path).await {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(e) => return Err(e.into()),
    };
    // Fast path: different sizes means changed.
    if origin_meta.len() != backup_meta.len() {
        return Ok(true);
    }
    // Content comparison for same-size files.
    let origin_bytes = fs::read(origin).await?;
    let backup_bytes = fs::read(backup_path).await?;
    Ok(origin_bytes != backup_bytes)
}

fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl FileHistoryState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start tracking a file for edit history.
    pub fn track_file(&mut self, path: PathBuf) {
        self.tracked_files.insert(path);
    }

    /// Whether a snapshot exists for a given message.
    pub fn can_restore(&self, message_id: &str) -> bool {
        self.snapshots.iter().any(|s| s.message_id == message_id)
    }

    /// Get the most recent backup for a file across all snapshots.
    pub fn latest_backup(&self, path: &Path) -> Option<&FileHistoryBackup> {
        self.snapshots
            .iter()
            .rev()
            .find_map(|s| s.tracked_file_backups.get(path))
    }

    /// Next version number for a file (1-based).
    fn next_version(&self, path: &Path) -> i32 {
        self.latest_backup(path).map_or(1, |b| b.version + 1)
    }

    /// Evict oldest snapshots beyond the cap.
    fn enforce_cap(&mut self) {
        while self.snapshots.len() > MAX_SNAPSHOTS {
            self.snapshots.remove(0);
        }
    }

    /// Track a file edit BEFORE writing. Creates a backup of pre-edit content.
    ///
    /// Three-phase: check state → async I/O → commit.
    /// Idempotent: safe to call multiple times per turn for the same file.
    pub async fn track_edit(
        &mut self,
        file_path: &Path,
        message_id: &str,
        config_home: &Path,
        session_id: &str,
    ) -> Result<()> {
        // Phase 1: Check if already tracked in current snapshot.
        if let Some(snapshot) = self.snapshots.last() {
            if snapshot.message_id == message_id
                && snapshot.tracked_file_backups.contains_key(file_path)
            {
                return Ok(());
            }
        }

        self.tracked_files.insert(file_path.to_path_buf());

        // Phase 2: Create backup of pre-edit content.
        let version = self.next_version(file_path);
        let backup_name = backup_file_name(file_path, version);
        let dest = resolve_backup_path(config_home, session_id, &backup_name);

        let backup_file_name = match fs::read(file_path).await {
            Ok(content) => {
                ensure_parent_dir(&dest).await?;
                fs::write(&dest, &content)
                    .await
                    .with_context(|| format!("writing backup to {}", dest.display()))?;
                // Preserve file permissions (TS: chmod(backupPath, srcStats.mode))
                #[cfg(unix)]
                if let Ok(meta) = fs::metadata(file_path).await {
                    let _ = fs::set_permissions(&dest, meta.permissions()).await;
                }
                Some(backup_name)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // File doesn't exist yet — will be created by the tool.
                None
            }
            Err(e) => return Err(e.into()),
        };

        // Phase 3: Commit to state.
        self.snapshot_sequence += 1;
        let is_new_file = backup_file_name.is_none();
        let backup = FileHistoryBackup {
            backup_file_name,
            version,
            backup_time: current_time_ms(),
        };

        if let Some(snapshot) = self.snapshots.last_mut() {
            if snapshot.message_id == message_id {
                snapshot
                    .tracked_file_backups
                    .insert(file_path.to_path_buf(), backup);
                // TS: tengu_file_history_track_edit_success
                tracing::info!(
                    target: "file_history",
                    event = "track_edit_success",
                    file = %file_path.display(),
                    version,
                    is_new_file,
                );
                return Ok(());
            }
        }

        // New snapshot — inherit backups from previous.
        let mut backups = self
            .snapshots
            .last()
            .map(|s| s.tracked_file_backups.clone())
            .unwrap_or_default();
        backups.insert(file_path.to_path_buf(), backup);

        self.snapshots.push(FileHistorySnapshot {
            message_id: message_id.to_string(),
            tracked_file_backups: backups,
            timestamp: current_time_ms(),
        });
        self.enforce_cap();
        // TS: tengu_file_history_track_edit_success
        tracing::info!(
            target: "file_history",
            event = "track_edit_success",
            file = %file_path.display(),
            version,
            is_new_file,
        );
        Ok(())
    }

    /// Create a full snapshot of all tracked files.
    ///
    /// Called after each user message. Backs up any files that changed since
    /// the last snapshot.
    pub async fn make_snapshot(
        &mut self,
        message_id: &str,
        config_home: &Path,
        session_id: &str,
    ) -> Result<()> {
        // Inherit backups from previous snapshot.
        let mut backups = self
            .snapshots
            .last()
            .map(|s| s.tracked_file_backups.clone())
            .unwrap_or_default();

        // Back up each tracked file if it changed.
        for file_path in &self.tracked_files.clone() {
            let prev_backup = self.latest_backup(file_path);
            let version = prev_backup.map_or(1, |b| b.version + 1);

            let needs_backup = if let Some(prev) = prev_backup {
                if let Some(ref bname) = prev.backup_file_name {
                    let bp = resolve_backup_path(config_home, session_id, bname);
                    origin_file_changed(file_path, &bp).await.unwrap_or(true)
                } else {
                    // Previous was null (didn't exist) — check if file now exists.
                    fs::metadata(file_path).await.is_ok()
                }
            } else {
                fs::metadata(file_path).await.is_ok()
            };

            if needs_backup {
                let bname = backup_file_name(file_path, version);
                let dest = resolve_backup_path(config_home, session_id, &bname);

                let backup_file = match fs::read(file_path).await {
                    Ok(content) => {
                        ensure_parent_dir(&dest).await?;
                        fs::write(&dest, &content).await?;
                        // Preserve file permissions (TS: chmod(backupPath, srcStats.mode))
                        #[cfg(unix)]
                        if let Ok(meta) = fs::metadata(file_path).await {
                            let _ = fs::set_permissions(&dest, meta.permissions()).await;
                        }
                        Some(bname)
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => return Err(e.into()),
                };

                backups.insert(
                    file_path.clone(),
                    FileHistoryBackup {
                        backup_file_name: backup_file,
                        version,
                        backup_time: current_time_ms(),
                    },
                );
            }
        }

        self.snapshot_sequence += 1;
        self.snapshots.push(FileHistorySnapshot {
            message_id: message_id.to_string(),
            tracked_file_backups: backups,
            timestamp: current_time_ms(),
        });
        self.enforce_cap();
        // TS: tengu_file_history_snapshot_success
        tracing::info!(
            target: "file_history",
            event = "snapshot_success",
            tracked_files = self.tracked_files.len(),
            snapshot_count = self.snapshots.len(),
        );
        Ok(())
    }

    /// Restore files to the state captured in a snapshot.
    pub async fn rewind(
        &self,
        message_id: &str,
        config_home: &Path,
        session_id: &str,
    ) -> Result<Vec<PathBuf>> {
        let snapshot = self
            .snapshots
            .iter()
            .rfind(|s| s.message_id == message_id)
            .context("no snapshot found for message")?;

        let changed = apply_snapshot(snapshot, config_home, session_id).await?;
        // TS: tengu_file_history_rewind_success
        tracing::info!(
            target: "file_history",
            event = "rewind_success",
            tracked_files = snapshot.tracked_file_backups.len(),
            files_changed = changed.len(),
        );
        Ok(changed)
    }

    /// Preview what `rewind` would change without actually modifying files.
    pub async fn get_diff_stats(
        &self,
        message_id: &str,
        config_home: &Path,
        session_id: &str,
    ) -> Result<DiffStats> {
        let snapshot = self
            .snapshots
            .iter()
            .rfind(|s| s.message_id == message_id)
            .context("no snapshot found for message")?;

        let mut stats = DiffStats::default();
        for (file_path, backup) in &snapshot.tracked_file_backups {
            let current = fs::read_to_string(file_path).await.ok();
            let backed_up = if let Some(ref bname) = backup.backup_file_name {
                let bp = resolve_backup_path(config_home, session_id, bname);
                fs::read_to_string(&bp).await.ok()
            } else {
                None
            };

            if current != backed_up {
                stats.files_changed.push(file_path.clone());
                // Use similar::TextDiff for proper line-level diff.
                // TS: diffLines() from npm 'diff' package.
                let cur = current.as_deref().unwrap_or("");
                let bak = backed_up.as_deref().unwrap_or("");
                let diff = similar::TextDiff::from_lines(bak, cur);
                for change in diff.iter_all_changes() {
                    let line_count = change.value().lines().count().max(1) as i64;
                    match change.tag() {
                        similar::ChangeTag::Insert => stats.insertions += line_count,
                        similar::ChangeTag::Delete => stats.deletions += line_count,
                        similar::ChangeTag::Equal => {}
                    }
                }
            }
        }
        Ok(stats)
    }

    /// Fast boolean check: has any tracked file changed since the given snapshot?
    pub async fn has_any_changes(
        &self,
        message_id: &str,
        config_home: &Path,
        session_id: &str,
    ) -> bool {
        let Some(snapshot) = self.snapshots.iter().rfind(|s| s.message_id == message_id) else {
            return false;
        };
        for (file_path, backup) in &snapshot.tracked_file_backups {
            if let Some(ref bname) = backup.backup_file_name {
                let bp = resolve_backup_path(config_home, session_id, bname);
                if origin_file_changed(file_path, &bp).await.unwrap_or(true) {
                    return true;
                }
            } else if fs::metadata(file_path).await.is_ok() {
                return true;
            }
        }
        false
    }

    /// Rebuild state from persisted snapshots (session resume).
    pub fn restore_from_snapshots(snapshots: Vec<FileHistorySnapshot>) -> Self {
        let mut tracked_files = HashSet::new();
        let mut max_version: i32 = 0;
        for snapshot in &snapshots {
            for (path, backup) in &snapshot.tracked_file_backups {
                tracked_files.insert(path.clone());
                max_version = max_version.max(backup.version);
            }
        }
        Self {
            snapshot_sequence: max_version as i64,
            tracked_files,
            snapshots,
        }
    }
}

/// Apply a snapshot: restore each tracked file from its backup.
/// Returns the list of files that were actually changed.
async fn apply_snapshot(
    snapshot: &FileHistorySnapshot,
    config_home: &Path,
    session_id: &str,
) -> Result<Vec<PathBuf>> {
    let mut changed = Vec::new();
    for (file_path, backup) in &snapshot.tracked_file_backups {
        match &backup.backup_file_name {
            Some(bname) => {
                let bp = resolve_backup_path(config_home, session_id, bname);
                if origin_file_changed(file_path, &bp).await.unwrap_or(true) {
                    let content = fs::read(&bp)
                        .await
                        .with_context(|| format!("reading backup {}", bp.display()))?;
                    ensure_parent_dir(file_path).await?;
                    fs::write(file_path, &content).await?;
                    // Restore file permissions (TS: chmod(filePath, backupStats.mode))
                    #[cfg(unix)]
                    if let Ok(meta) = fs::metadata(&bp).await {
                        let _ = fs::set_permissions(file_path, meta.permissions()).await;
                    }
                    changed.push(file_path.clone());
                }
            }
            None => {
                // File didn't exist at snapshot time — delete if it exists now.
                if fs::metadata(file_path).await.is_ok() {
                    fs::remove_file(file_path).await?;
                    changed.push(file_path.clone());
                }
            }
        }
    }
    Ok(changed)
}

/// Copy file history backups for session resume (hard-link with copy fallback).
pub async fn copy_file_history_for_resume(
    config_home: &Path,
    from_session: &str,
    to_session: &str,
) -> Result<i32> {
    let src_dir = backup_dir(config_home, from_session);
    let dst_dir = backup_dir(config_home, to_session);

    if !src_dir.exists() {
        return Ok(0);
    }
    fs::create_dir_all(&dst_dir).await?;

    let mut copied = 0i32;
    let mut entries = fs::read_dir(&src_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let src = entry.path();
        let dst = dst_dir.join(entry.file_name());
        if dst.exists() {
            continue;
        }
        // Try hard-link first (fast, no copy).
        if fs::hard_link(&src, &dst).await.is_err() {
            // Fallback to copy.
            if let Err(e) = fs::copy(&src, &dst).await {
                tracing::warn!("failed to copy backup {}: {e}", src.display());
                continue;
            }
        }
        copied += 1;
    }
    Ok(copied)
}

async fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "file_history.test.rs"]
mod tests;
