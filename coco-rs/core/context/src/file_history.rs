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
use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;

/// Maximum snapshots retained per session (oldest evicted).
const MAX_SNAPSHOTS: usize = 100;

/// IDE-bridge sink for file-update notifications.
///
/// Called by `make_snapshot` after a fresh snapshot is committed,
/// once per file whose backup name changed between the previous and
/// new snapshot. Mirrors TS `notifyVscodeSnapshotFilesUpdated`
/// (`fileHistory.ts:1054-1098`). The bridge implementation forwards
/// these to the IDE-side MCP so the editor can refresh open buffers.
#[async_trait]
pub trait FileUpdateSink: Send + Sync {
    async fn notify(
        &self,
        file_path: &Path,
        old_content: Option<String>,
        new_content: Option<String>,
    );
}

/// Persistence sink for file-history snapshots.
///
/// `record(message_id, snapshot, is_snapshot_update)` is called
/// every time `FileHistoryState::track_edit` or `make_snapshot`
/// mutates the in-memory snapshot vec. The implementer (typically
/// `coco-session::TranscriptStore`) appends a `file-history-snapshot`
/// entry to the JSONL transcript so that on resume, the state can
/// be rebuilt by replaying the chain.
///
/// TS: `recordFileHistorySnapshot` →
/// `Project::insertFileHistorySnapshot` → JSONL append. We pass
/// `serde_json::Value` instead of the typed snapshot to keep the
/// dependency edge from coco-context up to coco-session, not the
/// other way (coco-session would otherwise need to depend on
/// coco-context just for the typed shape).
#[async_trait]
pub trait FileHistorySnapshotSink: Send + Sync {
    async fn record(
        &self,
        message_id: &str,
        snapshot_json: serde_json::Value,
        is_snapshot_update: bool,
    );
}

/// Tracks file edits across the conversation for undo/snapshot capability.
#[derive(Default, Serialize, Deserialize)]
pub struct FileHistoryState {
    /// Ordered snapshots (newest last, max `MAX_SNAPSHOTS`).
    pub snapshots: Vec<FileHistorySnapshot>,
    /// Files currently being tracked.
    pub tracked_files: HashSet<PathBuf>,
    /// Monotonically increasing counter (activity signal).
    pub snapshot_sequence: i64,
    /// JSONL transcript writer. Skipped on serialization (sink isn't
    /// serializable; resume uses `restore_from_snapshots` to rebuild).
    #[serde(skip, default)]
    pub sink: Option<Arc<dyn FileHistorySnapshotSink>>,
    /// Optional IDE-bridge sink — called after each snapshot for
    /// every file whose content changed since the prior snapshot.
    #[serde(skip, default)]
    pub file_update_sink: Option<Arc<dyn FileUpdateSink>>,
}

impl Clone for FileHistoryState {
    fn clone(&self) -> Self {
        Self {
            snapshots: self.snapshots.clone(),
            tracked_files: self.tracked_files.clone(),
            snapshot_sequence: self.snapshot_sequence,
            sink: self.sink.clone(),
            file_update_sink: self.file_update_sink.clone(),
        }
    }
}

impl std::fmt::Debug for FileHistoryState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileHistoryState")
            .field("snapshots", &self.snapshots)
            .field("tracked_files", &self.tracked_files)
            .field("snapshot_sequence", &self.snapshot_sequence)
            .field("sink", &self.sink.as_ref().map(|_| "<sink>"))
            .field(
                "file_update_sink",
                &self.file_update_sink.as_ref().map(|_| "<sink>"),
            )
            .finish()
    }
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

/// Check if the origin file has changed compared to a backup.
///
/// Mirrors TS `compareStatsAndContent` (`fileHistory.ts:640-672`):
/// 1. Both missing → unchanged.
/// 2. One missing → changed.
/// 3. Different size → changed.
/// 4. Different mode (Unix) → changed (e.g. chmod-only edits).
/// 5. Origin mtime older than backup → unchanged (no need to read content).
/// 6. Else fall through to full byte-comparison.
async fn origin_file_changed(origin: &Path, backup_path: &Path) -> Result<bool> {
    let origin_meta = match fs::metadata(origin).await {
        Ok(m) => Some(m),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let backup_meta = match fs::metadata(backup_path).await {
        Ok(m) => Some(m),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let (origin_meta, backup_meta) = match (origin_meta, backup_meta) {
        (None, None) => return Ok(false),
        (Some(_), None) | (None, Some(_)) => return Ok(true),
        (Some(o), Some(b)) => (o, b),
    };

    // Different size means changed.
    if origin_meta.len() != backup_meta.len() {
        return Ok(true);
    }

    // chmod-only diffs: TS compares `mode` to detect permission flips.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if origin_meta.permissions().mode() != backup_meta.permissions().mode() {
            return Ok(true);
        }
    }

    // mtime short-circuit: if the origin was last modified before the
    // backup, the backup represents a newer state — nothing to do.
    // Avoids the byte read entirely on the common "not edited"
    // path.
    if let (Ok(origin_mtime), Ok(backup_mtime)) = (origin_meta.modified(), backup_meta.modified())
        && origin_mtime < backup_mtime
    {
        return Ok(false);
    }

    // Fall back to byte comparison.
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
        if let Some(snapshot) = self.snapshots.last()
            && snapshot.message_id == message_id
            && snapshot.tracked_file_backups.contains_key(file_path)
        {
            return Ok(());
        }

        self.tracked_files.insert(file_path.to_path_buf());

        // Phase 2: Create backup of pre-edit content.
        let version = self.next_version(file_path);
        let backup_name = backup_file_name(file_path, version);
        let dest = resolve_backup_path(config_home, session_id, &backup_name);

        let backup_file_name = match fs::read(file_path).await {
            Ok(content) => {
                ensure_parent_dir(&dest).await?;
                let size = content.len() as u64;
                fs::write(&dest, &content)
                    .await
                    .with_context(|| format!("writing backup to {}", dest.display()))?;
                // Preserve file permissions (TS: chmod(backupPath, srcStats.mode))
                #[cfg(unix)]
                if let Ok(meta) = fs::metadata(file_path).await {
                    let _ = fs::set_permissions(&dest, meta.permissions()).await;
                }
                coco_otel::events::emit_file_backup_created(
                    &file_path.display().to_string(),
                    version,
                    size,
                );
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

        if let Some(snapshot) = self.snapshots.last_mut()
            && snapshot.message_id == message_id
        {
            snapshot
                .tracked_file_backups
                .insert(file_path.to_path_buf(), backup);
            // TS: tengu_file_history_track_edit_success
            coco_otel::events::emit_file_track_edit_success(
                &file_path.display().to_string(),
                version,
                is_new_file,
            );
            // Persist the in-place update to the JSONL transcript.
            // TS: recordFileHistorySnapshot(messageId, snapshot, true)
            // — `true` means rewrite-existing in the chain builder.
            if let (Some(sink), Some(snap)) = (self.sink.clone(), self.snapshots.last().cloned())
                && let Ok(snap_json) = serde_json::to_value(&snap)
            {
                sink.record(message_id, snap_json, true).await;
            }
            return Ok(());
        }

        // New snapshot — inherit backups from previous.
        let mut backups = self
            .snapshots
            .last()
            .map(|s| s.tracked_file_backups.clone())
            .unwrap_or_default();
        backups.insert(file_path.to_path_buf(), backup);

        let new_snapshot = FileHistorySnapshot {
            message_id: message_id.to_string(),
            tracked_file_backups: backups,
            timestamp: current_time_ms(),
        };
        self.snapshots.push(new_snapshot.clone());
        self.enforce_cap();
        // TS: tengu_file_history_track_edit_success
        coco_otel::events::emit_file_track_edit_success(
            &file_path.display().to_string(),
            version,
            is_new_file,
        );
        if let Some(sink) = self.sink.clone()
            && let Ok(snap_json) = serde_json::to_value(&new_snapshot)
        {
            // First time we're writing this message_id — `false`
            // appends; later updates within the same turn will use
            // `true` via the branch above.
            sink.record(message_id, snap_json, false).await;
        }
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
        // Capture the prior snapshot before mutating so we can
        // compute file-update diffs for the IDE-bridge sink.
        let prior_snapshot = self.snapshots.last().cloned();
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
                        let size = content.len() as u64;
                        fs::write(&dest, &content).await?;
                        // Preserve file permissions (TS: chmod(backupPath, srcStats.mode))
                        #[cfg(unix)]
                        if let Ok(meta) = fs::metadata(file_path).await {
                            let _ = fs::set_permissions(&dest, meta.permissions()).await;
                        }
                        coco_otel::events::emit_file_backup_created(
                            &file_path.display().to_string(),
                            version,
                            size,
                        );
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
        let new_snapshot = FileHistorySnapshot {
            message_id: message_id.to_string(),
            tracked_file_backups: backups,
            timestamp: current_time_ms(),
        };
        self.snapshots.push(new_snapshot.clone());
        self.enforce_cap();
        // TS: tengu_file_history_snapshot_success
        coco_otel::events::emit_file_snapshot_success(
            self.tracked_files.len(),
            self.snapshots.len(),
        );
        // TS: recordFileHistorySnapshot(messageId, snapshot, false)
        // — `false` appends a fresh entry; resume rebuilds the chain
        // by reading these in order.
        if let Some(sink) = self.sink.clone()
            && let Ok(snap_json) = serde_json::to_value(&new_snapshot)
        {
            sink.record(message_id, snap_json, false).await;
        }

        // IDE-bridge: notify per-file content updates.
        // TS: notifyVscodeSnapshotFilesUpdated (`fileHistory.ts:1054`).
        if let Some(file_sink) = self.file_update_sink.clone() {
            for (path, new_backup) in &new_snapshot.tracked_file_backups {
                let old_backup = prior_snapshot
                    .as_ref()
                    .and_then(|s| s.tracked_file_backups.get(path));
                // Skip when name + version unchanged (same backup blob,
                // no edit between snapshots).
                if old_backup.map(|b| (&b.backup_file_name, b.version))
                    == Some((&new_backup.backup_file_name, new_backup.version))
                {
                    continue;
                }
                let old_content = match old_backup.and_then(|b| b.backup_file_name.as_ref()) {
                    Some(name) => {
                        let bp = resolve_backup_path(config_home, session_id, name);
                        fs::read_to_string(&bp).await.ok()
                    }
                    None => None,
                };
                let new_content = match new_backup.backup_file_name.as_ref() {
                    Some(name) => {
                        let bp = resolve_backup_path(config_home, session_id, name);
                        fs::read_to_string(&bp).await.ok()
                    }
                    None => None,
                };
                if old_content != new_content {
                    file_sink.notify(path, old_content, new_content).await;
                }
            }
        }
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

        let changed = match apply_snapshot(self, snapshot, config_home, session_id).await {
            Ok(c) => c,
            Err(e) => {
                coco_otel::events::emit_file_rewind_failed(&e.to_string());
                return Err(e);
            }
        };
        // TS: tengu_file_history_rewind_success
        coco_otel::events::emit_file_rewind_success(
            snapshot.tracked_file_backups.len(),
            changed.len(),
        );
        Ok(changed)
    }

    /// Look up the first-version backup for a tracked file.
    ///
    /// Returns:
    /// - `Some(Some(name))` — restore from this backup
    /// - `Some(None)` — file did not exist at v1 (delete it on rewind)
    /// - `None` — no v1 entry for this path; cannot resolve
    ///
    /// TS: `getBackupFileNameFirstVersion` (`fileHistory.ts:847-862`).
    /// Used by `apply_snapshot` and `get_diff_stats` when the target
    /// snapshot has no entry for a file that became tracked later
    /// (e.g. file first edited in turn 5, rewinding to turn 7's
    /// snapshot which inherits backups but the entry only exists if
    /// the file was modified in turns 5–7).
    pub fn first_version_backup_name(&self, path: &Path) -> Option<Option<String>> {
        for snapshot in &self.snapshots {
            if let Some(backup) = snapshot.tracked_file_backups.get(path)
                && backup.version == 1
            {
                return Some(backup.backup_file_name.clone());
            }
        }
        None
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
        // Walk `tracked_files`, falling back to v1 backup for files
        // not in the target snapshot's map. Mirrors `apply_snapshot`'s
        // coverage. TS: `fileHistoryGetDiffStats` (`fileHistory.ts:414`).
        for file_path in &self.tracked_files {
            let backup_file_name = match snapshot.tracked_file_backups.get(file_path) {
                Some(b) => b.backup_file_name.clone(),
                None => match self.first_version_backup_name(file_path) {
                    Some(name) => name,
                    None => continue,
                },
            };

            let current = fs::read_to_string(file_path).await.ok();
            let backed_up = if let Some(ref bname) = backup_file_name {
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
                    // Match TS `diffLines` line counting: count newline
                    // boundaries in the change value, treating a
                    // chunk without a trailing newline as +1 line.
                    let v = change.value();
                    let nl = v.bytes().filter(|&b| b == b'\n').count() as i64;
                    let line_count = if v.ends_with('\n') || v.is_empty() {
                        nl
                    } else {
                        nl + 1
                    };
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
        // Walk tracked_files with v1 fallback (TS parity).
        for file_path in &self.tracked_files {
            let backup_file_name = match snapshot.tracked_file_backups.get(file_path) {
                Some(b) => b.backup_file_name.clone(),
                None => match self.first_version_backup_name(file_path) {
                    Some(name) => name,
                    None => continue,
                },
            };
            if let Some(ref bname) = backup_file_name {
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

    /// Install a JSONL persistence sink. Calls to `track_edit` and
    /// `make_snapshot` after this will append to the sink.
    pub fn with_sink(mut self, sink: Arc<dyn FileHistorySnapshotSink>) -> Self {
        self.sink = Some(sink);
        self
    }

    /// Install or replace the snapshot sink in-place.
    pub fn set_sink(&mut self, sink: Arc<dyn FileHistorySnapshotSink>) {
        self.sink = Some(sink);
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
            sink: None,
            file_update_sink: None,
        }
    }

    /// Install an IDE-bridge file-update sink. After each
    /// `make_snapshot`, files whose backup name changed will be
    /// notified through this sink. Mirrors TS
    /// `notifyVscodeSnapshotFilesUpdated`.
    pub fn set_file_update_sink(&mut self, sink: Arc<dyn FileUpdateSink>) {
        self.file_update_sink = Some(sink);
    }
}

/// Apply a snapshot: restore each tracked file from its backup.
/// Returns the list of files that were actually changed.
async fn apply_snapshot(
    state: &FileHistoryState,
    snapshot: &FileHistorySnapshot,
    config_home: &Path,
    session_id: &str,
) -> Result<Vec<PathBuf>> {
    let mut changed = Vec::new();
    // Iterate the full tracked-files set (not just snapshot's
    // backups). For files the snapshot has no entry for, fall back
    // to their first-version backup so files first edited mid-
    // conversation are restorable to any earlier rewind point.
    // TS: `applySnapshot` walks `state.trackedFiles` and uses
    // `getBackupFileNameFirstVersion` (`fileHistory.ts:537-559`).
    for file_path in &state.tracked_files {
        let backup_file_name = match snapshot.tracked_file_backups.get(file_path) {
            Some(backup) => backup.backup_file_name.clone(),
            None => match state.first_version_backup_name(file_path) {
                Some(name) => name,
                None => {
                    tracing::warn!(
                        target: "file_history",
                        path = %file_path.display(),
                        "no v1 backup; cannot restore — leaving file untouched",
                    );
                    continue;
                }
            },
        };

        match backup_file_name {
            Some(bname) => {
                let bp = resolve_backup_path(config_home, session_id, &bname);
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
