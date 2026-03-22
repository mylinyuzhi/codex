use std::collections::HashSet;
use std::collections::VecDeque;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use cocode_git::CreateGhostCommitOptions;
use cocode_git::GhostCommit;
use cocode_git::GhostSnapshotConfig;
use cocode_git::RestoreGhostCommitOptions;
use serde::Deserialize;
use serde::Serialize;
use similar::TextDiff;
use snafu::ResultExt;
use snafu::ensure;
use tokio::sync::Mutex;
use tokio::sync::RwLock;

use crate::Result;
use crate::error::file_backup_error;

use crate::backup::BackupEntry;
use crate::backup::FileBackupStore;

/// Snapshot of a single turn, combining Tier 1 (file backup) and Tier 2 (ghost commit).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnSnapshot {
    /// Unique turn identifier.
    pub turn_id: String,
    /// Turn number (1-indexed).
    pub turn_number: i32,
    /// Tier 2: ghost commit (only in git repos).
    pub ghost_commit: Option<GhostCommit>,
    /// Tier 1: file backup entries.
    pub file_backups: Vec<BackupEntry>,
}

/// Information about a completed rewind, consumed by system reminder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewindInfo {
    /// The turn number that was rewound.
    pub rewound_turn_number: i32,
    /// Ghost commit ID used for restoration (if any).
    pub restored_commit_id: Option<String>,
    /// Number of files restored.
    pub restored_file_count: i32,
    /// The rewind mode used.
    pub mode: RewindMode,
}

/// Result of a rewind operation.
#[derive(Debug, Clone)]
pub struct RewindResult {
    /// Turn number that was rewound.
    pub rewound_turn: i32,
    /// Files that were restored.
    pub restored_files: Vec<PathBuf>,
    /// Whether git restore (Tier 2) was used.
    pub used_git_restore: bool,
    /// The mode used for this rewind.
    pub mode: RewindMode,
}

/// Re-export from protocol for convenience.
pub use cocode_protocol::RewindMode;

/// Summary of an available checkpoint for display in the rewind selector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointInfo {
    /// The turn number.
    pub turn_number: i32,
    /// Number of files modified in this turn.
    pub file_count: i32,
    /// List of file paths modified in this turn.
    pub modified_files: Vec<PathBuf>,
    /// Whether a ghost commit is available for this turn.
    pub has_ghost_commit: bool,
}

/// Configuration for ghost snapshot behavior.
#[derive(Debug, Clone, Default)]
pub struct GhostConfig {
    pub ghost_snapshot: GhostSnapshotConfig,
}

/// Default maximum number of snapshots to retain.
pub const DEFAULT_MAX_SNAPSHOTS: usize = 50;

/// Diff statistics for a hypothetical rewind operation (dry-run).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DryRunDiffStats {
    /// Number of files that would change.
    pub files_changed: i32,
    /// Total lines inserted (across all files).
    pub insertions: i32,
    /// Total lines removed (across all files).
    pub deletions: i32,
}

/// Unified two-tier snapshot manager.
///
/// - Tier 1 (FileBackupStore): Per-file backups before Write/Edit tools. Works everywhere.
/// - Tier 2 (Ghost Commits): Detached git commits capturing full working tree. Git repos only.
///
/// Rewind strategy:
/// - Git repo with ghost commit: Restore ghost commit (covers all changes including bash).
/// - Non-git / no ghost commit: Restore from file backups (covers tool-modified files only).
pub struct SnapshotManager {
    backup_store: Arc<FileBackupStore>,
    cwd: PathBuf,
    is_git_repo: bool,
    ghost_config: GhostConfig,
    /// Maximum snapshots to retain (oldest trimmed on finalize).
    max_snapshots: usize,
    /// Stack of turn snapshots (newest at the end).
    /// Uses `RwLock` because reads (list_checkpoints, has_snapshots, etc.)
    /// far outnumber writes (finalize, rewind).
    snapshot_stack: RwLock<VecDeque<TurnSnapshot>>,
    /// Rewind info for system reminder consumption (consumed once).
    last_rewind: Mutex<Option<RewindInfo>>,
    /// Compaction boundary turn number — cannot rewind past this.
    compaction_boundary: Mutex<Option<i32>>,
}

impl SnapshotManager {
    /// Create a new snapshot manager with default retention policy.
    pub fn new(
        backup_store: Arc<FileBackupStore>,
        cwd: PathBuf,
        is_git_repo: bool,
        ghost_config: GhostConfig,
    ) -> Self {
        Self::with_max_snapshots(
            backup_store,
            cwd,
            is_git_repo,
            ghost_config,
            DEFAULT_MAX_SNAPSHOTS,
        )
    }

    /// Create a new snapshot manager with a custom retention limit.
    pub fn with_max_snapshots(
        backup_store: Arc<FileBackupStore>,
        cwd: PathBuf,
        is_git_repo: bool,
        ghost_config: GhostConfig,
        max_snapshots: usize,
    ) -> Self {
        Self {
            backup_store,
            cwd,
            is_git_repo,
            ghost_config,
            max_snapshots,
            snapshot_stack: RwLock::new(VecDeque::new()),
            last_rewind: Mutex::new(None),
            compaction_boundary: Mutex::new(None),
        }
    }

    /// Get a reference to the backing FileBackupStore.
    pub fn backup_store(&self) -> &Arc<FileBackupStore> {
        &self.backup_store
    }

    /// Whether this manager operates in a git repository.
    pub fn is_git_repo(&self) -> bool {
        self.is_git_repo
    }

    /// Start tracking a new turn. Call at the beginning of each turn.
    ///
    /// Sets the current turn ID on the backup store and optionally creates
    /// a ghost commit (Tier 2) in the background.
    pub async fn start_turn_snapshot(
        &self,
        turn_id: &str,
        turn_number: i32,
        create_ghost: bool,
    ) -> Option<GhostCommit> {
        self.backup_store.set_current_turn(turn_id).await;

        if !create_ghost || !self.is_git_repo {
            return None;
        }

        // Create ghost commit in a blocking task to avoid blocking the tokio runtime.
        let cwd = self.cwd.clone();
        let ghost_snapshot = self.ghost_config.ghost_snapshot.clone();
        let message = format!("snapshot turn {turn_number}");

        match tokio::task::spawn_blocking(move || {
            let opts = CreateGhostCommitOptions::new(&cwd)
                .message(&message)
                .ghost_snapshot(ghost_snapshot);
            cocode_git::create_ghost_commit(&opts)
        })
        .await
        {
            Ok(Ok(gc)) => {
                tracing::debug!(
                    turn_number,
                    commit_id = gc.id(),
                    "Created ghost commit for turn"
                );
                Some(gc)
            }
            Ok(Err(e)) => {
                tracing::warn!("Failed to create ghost commit: {e}");
                None
            }
            Err(e) => {
                tracing::warn!("Ghost commit task panicked: {e}");
                None
            }
        }
    }

    /// Finalize a turn snapshot after all tools have executed.
    ///
    /// Collects file backup entries for this turn, pushes the snapshot onto
    /// the stack, and trims the oldest snapshots if the stack exceeds
    /// `max_snapshots`. Trimmed backup blobs are cleaned up.
    pub async fn finalize_turn_snapshot(
        &self,
        turn_id: &str,
        turn_number: i32,
        ghost_commit: Option<GhostCommit>,
    ) {
        let file_backups = self.backup_store.entries_for_turn(turn_id).await;

        let snapshot = TurnSnapshot {
            turn_id: turn_id.to_string(),
            turn_number,
            ghost_commit,
            file_backups,
        };

        let trimmed = {
            let mut stack = self.snapshot_stack.write().await;
            stack.push_back(snapshot);

            // Trim oldest snapshots beyond retention limit.
            let mut removed = Vec::new();
            while stack.len() > self.max_snapshots {
                if let Some(old) = stack.pop_front() {
                    removed.push(old);
                }
            }
            removed
        };

        // Clean up backup blobs for trimmed snapshots (outside lock).
        for old in &trimmed {
            self.backup_store.remove_turn(&old.turn_id).await;
        }
    }

    /// Rewind the last turn, restoring files to their pre-turn state.
    ///
    /// Returns `Err` if:
    /// - No snapshots exist
    /// - The snapshot is before the compaction boundary
    pub async fn rewind_last_turn(&self) -> Result<RewindResult> {
        self.rewind_to_turn_with_mode(None, RewindMode::CodeAndConversation)
            .await
    }

    /// Rewind to a specific turn number with a given mode.
    ///
    /// Pops all snapshots at or after `target_turn` and restores files
    /// for the earliest one (when mode includes code restoration).
    ///
    /// If `target_turn` is `None`, rewinds the last turn only.
    pub async fn rewind_to_turn_with_mode(
        &self,
        target_turn: Option<i32>,
        mode: RewindMode,
    ) -> Result<RewindResult> {
        // Lock order: compaction_boundary → snapshot_stack (must match set_compaction_boundary)
        let boundary = *self.compaction_boundary.lock().await;

        let snapshots_to_rewind = {
            let mut stack = self.snapshot_stack.write().await;
            let target = match target_turn {
                Some(t) => t,
                None => match stack.back() {
                    Some(s) => s.turn_number,
                    None => {
                        return file_backup_error::InvalidStateSnafu {
                            message: "No snapshots available to rewind".to_string(),
                        }
                        .fail();
                    }
                },
            };

            // Check compaction boundary
            if let Some(b) = boundary {
                ensure!(
                    target > b,
                    file_backup_error::InvalidStateSnafu {
                        message: format!("Cannot rewind past compaction boundary (turn {b})"),
                    }
                );
            }

            // Pop all snapshots at or after target turn
            let split_idx = stack.iter().position(|s| s.turn_number >= target);
            match split_idx {
                Some(idx) => stack.drain(idx..).collect::<Vec<_>>(),
                None => {
                    return file_backup_error::InvalidStateSnafu {
                        message: format!("No snapshots found at or after turn {target}"),
                    }
                    .fail();
                }
            }
        };

        if snapshots_to_rewind.is_empty() {
            return file_backup_error::InvalidStateSnafu {
                message: "No snapshots available to rewind".to_string(),
            }
            .fail();
        }

        let earliest_turn = snapshots_to_rewind[0].turn_number;
        let restore_code = mode != RewindMode::ConversationOnly;

        // Attempt file restoration. On failure, push snapshots back onto the
        // stack so the user can retry.
        let restore_result = self.restore_files(&snapshots_to_rewind, restore_code).await;

        let (all_restored_files, used_git) = match restore_result {
            Ok(outcome) => outcome,
            Err(e) => {
                // Push snapshots back — the rewind failed, keep them available.
                let mut stack = self.snapshot_stack.write().await;
                for snap in snapshots_to_rewind {
                    stack.push_back(snap);
                }
                // Re-sort to maintain turn order after re-insertion.
                let mut vec: Vec<_> = stack.drain(..).collect();
                vec.sort_by_key(|s| s.turn_number);
                stack.extend(vec);
                return Err(e);
            }
        };

        // Clean up backup entries for all rewound turns
        for snap in &snapshots_to_rewind {
            self.backup_store.remove_turn(&snap.turn_id).await;
        }

        let result = RewindResult {
            rewound_turn: earliest_turn,
            restored_files: all_restored_files.clone(),
            used_git_restore: used_git,
            mode,
        };

        // Set rewind info for system reminder.
        // Only record restored_commit_id when git restore was actually used,
        // not merely because a ghost commit exists in the snapshots.
        let restored_commit_id = if used_git {
            snapshots_to_rewind
                .iter()
                .find_map(|s| s.ghost_commit.as_ref().map(|gc| gc.id().to_string()))
        } else {
            None
        };
        *self.last_rewind.lock().await = Some(RewindInfo {
            rewound_turn_number: earliest_turn,
            restored_commit_id,
            restored_file_count: all_restored_files.len() as i32,
            mode,
        });

        Ok(result)
    }

    /// Restore files from the given snapshots.
    ///
    /// Returns `(restored_files, used_git)` on success.
    async fn restore_files(
        &self,
        snapshots: &[TurnSnapshot],
        restore_code: bool,
    ) -> Result<(Vec<PathBuf>, bool)> {
        if !restore_code {
            return Ok((Vec::new(), false));
        }

        let mut seen = HashSet::new();
        let mut all_restored_files = Vec::new();
        let mut used_git = false;

        // Use Tier 2 only if the earliest snapshot (the rewind target) has a
        // ghost commit. If a later snapshot has one but the earliest doesn't,
        // restoring that later ghost would leave the earliest turn's changes
        // in place, which is incorrect.
        let ghost_snapshot = snapshots.first().filter(|s| s.ghost_commit.is_some());

        if self.is_git_repo
            && let Some(gs) = ghost_snapshot
            && let Some(ref gc) = gs.ghost_commit
        {
            // Tier 2: git restore to earliest snapshot
            let cwd = self.cwd.clone();
            let ghost_snapshot_config = self.ghost_config.ghost_snapshot.clone();
            let gc_clone = gc.clone();
            tokio::task::spawn_blocking(move || {
                let opts =
                    RestoreGhostCommitOptions::new(&cwd).ghost_snapshot(ghost_snapshot_config);
                cocode_git::restore_ghost_commit_with_options(&opts, &gc_clone)
            })
            .await
            .context(file_backup_error::TaskJoinSnafu {
                message: "ghost commit restore task panicked".to_string(),
            })?
            .map_err(|e| {
                file_backup_error::GitSnafu {
                    message: format!("restoring ghost commit: {e}"),
                }
                .build()
            })?;

            // Collect all modified file paths across all rewound turns
            for snap in snapshots {
                for entry in &snap.file_backups {
                    if seen.insert(entry.original_path.clone()) {
                        all_restored_files.push(entry.original_path.clone());
                    }
                }
            }
            used_git = true;
        }

        // Tier 1 fallback: non-git repos, or git repos without a usable ghost commit.
        if !used_git {
            self.restore_files_tier1(snapshots, &mut seen, &mut all_restored_files)
                .await?;
        }

        Ok((all_restored_files, used_git))
    }

    /// Restore files using Tier 1 (file backups) for all given snapshots.
    ///
    /// When multiple turns are rewound, the same file may appear in several
    /// snapshots. Only the **earliest** turn's backup (the pre-change state)
    /// is correct for restoration. Later turns' backups are intermediate
    /// states that would leave the file in the wrong state.
    async fn restore_files_tier1(
        &self,
        snapshots: &[TurnSnapshot],
        seen: &mut HashSet<PathBuf>,
        all_restored: &mut Vec<PathBuf>,
    ) -> Result<()> {
        // Collect all backup entries, keeping only the first (earliest) per path.
        let mut entries_to_restore: Vec<&BackupEntry> = Vec::new();
        let mut entry_seen: HashSet<&PathBuf> = HashSet::new();
        for snap in snapshots {
            for entry in &snap.file_backups {
                if entry_seen.insert(&entry.original_path) {
                    entries_to_restore.push(entry);
                }
            }
        }

        // Restore each deduplicated entry via the backup store.
        for entry in entries_to_restore {
            let restored = self.backup_store.restore_entry(entry).await.map_err(|e| {
                file_backup_error::InvalidStateSnafu {
                    message: format!("restoring file backup: {e}"),
                }
                .build()
            })?;
            if restored && seen.insert(entry.original_path.clone()) {
                all_restored.push(entry.original_path.clone());
            }
        }
        Ok(())
    }

    /// List all available checkpoints for the rewind selector.
    pub async fn list_checkpoints(&self) -> Vec<CheckpointInfo> {
        let stack = self.snapshot_stack.read().await;
        stack
            .iter()
            .map(|snap| CheckpointInfo {
                turn_number: snap.turn_number,
                file_count: snap.file_backups.len() as i32,
                modified_files: snap
                    .file_backups
                    .iter()
                    .map(|e| e.original_path.clone())
                    .collect(),
                has_ghost_commit: snap.ghost_commit.is_some(),
            })
            .collect()
    }

    /// Take the last rewind info (consumed once by system reminder).
    pub async fn take_rewind_info(&self) -> Option<RewindInfo> {
        self.last_rewind.lock().await.take()
    }

    /// Set the compaction boundary. Snapshots at or before this turn cannot be rewound.
    pub async fn set_compaction_boundary(&self, turn_number: i32) {
        *self.compaction_boundary.lock().await = Some(turn_number);
        // Clean up old snapshots
        let mut stack = self.snapshot_stack.write().await;
        stack.retain(|s| s.turn_number > turn_number);
    }

    /// Check if any snapshots are available for rewind.
    pub async fn has_snapshots(&self) -> bool {
        !self.snapshot_stack.read().await.is_empty()
    }

    /// Get the turn number of the most recent snapshot (for UI display).
    pub async fn last_snapshot_turn(&self) -> Option<i32> {
        self.snapshot_stack
            .read()
            .await
            .back()
            .map(|s| s.turn_number)
    }

    /// Compute diff stats for a hypothetical rewind to `target_turn` without modifying files.
    ///
    /// Reads files on disk and compares with their backup state. Returns aggregate
    /// line-level diff statistics used by the rewind selector for preview.
    ///
    /// Binary files are detected via UTF-8 validation. They count toward
    /// `files_changed` but not `insertions`/`deletions` (no line-level diff).
    pub async fn dry_run_diff_stats(&self, target_turn: i32) -> Result<DryRunDiffStats> {
        // Collect backup entries from snapshots at or after target_turn.
        let entries: Vec<BackupEntry> = {
            let stack = self.snapshot_stack.read().await;
            stack
                .iter()
                .filter(|s| s.turn_number >= target_turn)
                .flat_map(|s| s.file_backups.iter().cloned())
                .collect()
        };

        // Deduplicate by path — keep the first (earliest) entry per path,
        // which represents the pre-change state to restore to.
        let mut seen_paths = HashSet::new();
        let unique_entries: Vec<&BackupEntry> = entries
            .iter()
            .filter(|e| seen_paths.insert(e.original_path.clone()))
            .collect();

        let mut stats = DryRunDiffStats::default();

        for entry in &unique_entries {
            let current_exists = tokio::fs::try_exists(&entry.original_path)
                .await
                .unwrap_or(false);

            if !entry.existed_before {
                // File was newly created — rewind would delete it.
                if current_exists {
                    if let DiffContent::Text(text) = read_file_for_diff(&entry.original_path).await
                    {
                        stats.deletions += text.lines().count() as i32;
                    }
                    // Binary or text — still a changed file.
                    stats.files_changed += 1;
                }
                continue;
            }

            // File existed before — compare current vs backup.
            let backup = if !entry.backup_filename.is_empty() {
                let blob_path = self.backup_store.backup_dir().join(&entry.backup_filename);
                read_file_for_diff(&blob_path).await
            } else {
                DiffContent::Missing
            };

            let current = if current_exists {
                read_file_for_diff(&entry.original_path).await
            } else {
                DiffContent::Missing
            };

            match (&current, &backup) {
                (DiffContent::Text(cur), DiffContent::Text(bak)) => {
                    if cur != bak {
                        let diff = TextDiff::from_lines(cur.as_str(), bak.as_str());
                        for change in diff.iter_all_changes() {
                            match change.tag() {
                                similar::ChangeTag::Insert => stats.insertions += 1,
                                similar::ChangeTag::Delete => stats.deletions += 1,
                                similar::ChangeTag::Equal => {}
                            }
                        }
                        stats.files_changed += 1;
                    }
                }
                // Binary files: count as changed if sizes differ, no line stats.
                (DiffContent::Binary(cur_sz), DiffContent::Binary(bak_sz)) => {
                    if cur_sz != bak_sz {
                        stats.files_changed += 1;
                    }
                }
                // Mixed text/binary or one side missing — count as changed.
                (DiffContent::Missing, DiffContent::Text(bak)) => {
                    stats.insertions += bak.lines().count() as i32;
                    stats.files_changed += 1;
                }
                (DiffContent::Text(cur), DiffContent::Missing) => {
                    stats.deletions += cur.lines().count() as i32;
                    stats.files_changed += 1;
                }
                (DiffContent::Missing, DiffContent::Binary(_)) => {
                    stats.files_changed += 1;
                }
                (DiffContent::Binary(_), DiffContent::Missing) => {
                    stats.files_changed += 1;
                }
                (DiffContent::Text(_), DiffContent::Binary(_))
                | (DiffContent::Binary(_), DiffContent::Text(_)) => {
                    // Type changed between text and binary — always a change.
                    stats.files_changed += 1;
                }
                (DiffContent::Missing, DiffContent::Missing) => {
                    // Both gone — no change needed.
                }
            }
        }

        Ok(stats)
    }

    /// Serialize snapshots for session persistence.
    pub async fn serialize_snapshots(&self) -> Result<String> {
        let stack = self.snapshot_stack.read().await;
        // Serialize as Vec for compatibility.
        let vec: Vec<&TurnSnapshot> = stack.iter().collect();
        serde_json::to_string_pretty(&vec).context(file_backup_error::JsonSnafu {
            message: "serializing snapshots".to_string(),
        })
    }

    /// Restore snapshots from persisted data.
    pub async fn restore_snapshots(&self, json: &str) -> Result<()> {
        let vec: Vec<TurnSnapshot> =
            serde_json::from_str(json).context(file_backup_error::JsonSnafu {
                message: "deserializing snapshots".to_string(),
            })?;
        *self.snapshot_stack.write().await = VecDeque::from(vec);
        Ok(())
    }
}

/// Content classification for diff stats computation.
///
/// Distinguishes text (line-diffable) from binary (size-only comparison)
/// from missing (file doesn't exist / unreadable).
enum DiffContent {
    /// UTF-8 text content — supports line-level diffing.
    Text(String),
    /// Binary content — only byte size is available.
    Binary(usize),
    /// File doesn't exist or couldn't be read.
    Missing,
}

/// Read a file for diff stats, classifying as text or binary.
///
/// Reads raw bytes and attempts UTF-8 conversion. Falls back to
/// `Binary(size)` for non-UTF-8 content so binary files are still
/// counted toward `files_changed`.
async fn read_file_for_diff(path: &Path) -> DiffContent {
    match tokio::fs::read(path).await {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => DiffContent::Text(text),
            Err(e) => DiffContent::Binary(e.into_bytes().len()),
        },
        Err(_) => DiffContent::Missing,
    }
}

#[cfg(test)]
#[path = "snapshot.test.rs"]
mod tests;
