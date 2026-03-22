use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use snafu::ResultExt;
use tokio::sync::Mutex;

use crate::Result;
use crate::error::file_backup_error;

/// Maximum file size to backup (10 MiB).
const MAX_BACKUP_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// A single backup entry recording the original state of a file before modification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupEntry {
    /// Absolute path to the original file.
    pub original_path: PathBuf,
    /// SHA256 hex hash of the file content (for dedup).
    pub content_hash: String,
    /// Filename in the backup directory (e.g. `{hash16}@v1`).
    pub backup_filename: String,
    /// Whether the file existed before modification (false = newly created).
    pub existed_before: bool,
    /// Turn ID when this backup was created.
    pub turn_id: String,
    /// Original file permissions (Unix mode bits). None for non-existent files
    /// or non-Unix platforms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_mode: Option<u32>,
}

/// Index tracking all backups across turns with content deduplication.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackupIndex {
    /// Backups grouped by turn_id.
    pub turns: HashMap<String, Vec<BackupEntry>>,
    /// Content dedup: content_hash -> backup_filename.
    pub content_map: HashMap<String, String>,
    /// Next version counter per path-hash prefix for unique filenames.
    next_version: HashMap<String, u32>,
}

/// Manages file backups before tool modifications (Tier 1 of the rewind system).
///
/// Each Write/Edit tool execution triggers a backup of the original file content.
/// Files are deduplicated by SHA256 hash to avoid storing identical content twice.
pub struct FileBackupStore {
    backup_dir: PathBuf,
    current_turn_id: Mutex<String>,
    /// Dedup: (turn_id, abs_path) - same file backed up at most once per turn.
    backed_up_this_turn: Mutex<HashSet<(String, PathBuf)>>,
    index: Mutex<BackupIndex>,
}

impl FileBackupStore {
    /// Create a new backup store for the given session.
    pub async fn new(session_dir: &Path, session_id: &str) -> Result<Self> {
        let backup_dir = session_dir.join(session_id).join("file-backups");
        tokio::fs::create_dir_all(&backup_dir)
            .await
            .context(file_backup_error::IoSnafu {
                message: "creating file-backup directory".to_string(),
            })?;

        let index = Self::load_index(&backup_dir).await;

        Ok(Self {
            backup_dir,
            current_turn_id: Mutex::new(String::new()),
            backed_up_this_turn: Mutex::new(HashSet::new()),
            index: Mutex::new(index),
        })
    }

    /// Create a backup store with an explicit backup directory (for testing).
    pub async fn with_dir(backup_dir: PathBuf) -> Result<Self> {
        tokio::fs::create_dir_all(&backup_dir)
            .await
            .context(file_backup_error::IoSnafu {
                message: "creating file-backup directory".to_string(),
            })?;

        let index = Self::load_index(&backup_dir).await;

        Ok(Self {
            backup_dir,
            current_turn_id: Mutex::new(String::new()),
            backed_up_this_turn: Mutex::new(HashSet::new()),
            index: Mutex::new(index),
        })
    }

    /// Set the current turn ID and clear per-turn dedup state.
    pub async fn set_current_turn(&self, turn_id: &str) {
        let mut tid = self.current_turn_id.lock().await;
        *tid = turn_id.to_string();
        self.backed_up_this_turn.lock().await.clear();
    }

    /// Backup the file at `path` before it is modified by a tool.
    ///
    /// - Skips if already backed up this turn for the same path.
    /// - Skips files larger than 10 MiB.
    /// - Records non-existent files as `existed_before: false` (for deletion on rewind).
    pub async fn backup_before_modify(&self, path: &Path) -> Result<()> {
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .context(file_backup_error::IoSnafu {
                    message: "getting current_dir".to_string(),
                })?
                .join(path)
        };

        let turn_id = self.current_turn_id.lock().await.clone();
        if turn_id.is_empty() {
            return Ok(());
        }

        // Dedup: skip if already backed up this turn
        {
            let mut backed = self.backed_up_this_turn.lock().await;
            let key = (turn_id.clone(), abs_path.clone());
            if backed.contains(&key) {
                return Ok(());
            }
            backed.insert(key);
        }

        // Check if file exists
        let existed = tokio::fs::try_exists(&abs_path).await.unwrap_or(false);

        if !existed {
            // Record that this file didn't exist (will be deleted on rewind)
            let entry = BackupEntry {
                original_path: abs_path,
                content_hash: String::new(),
                backup_filename: String::new(),
                existed_before: false,
                turn_id: turn_id.clone(),
                file_mode: None,
            };
            let mut idx = self.index.lock().await;
            idx.turns.entry(turn_id).or_default().push(entry);
            self.save_index_locked(&idx).await;
            return Ok(());
        }

        // Check file size and capture permissions
        let metadata =
            tokio::fs::metadata(&abs_path)
                .await
                .context(file_backup_error::IoSnafu {
                    message: "reading file metadata".to_string(),
                })?;
        #[cfg(unix)]
        let file_mode = {
            use std::os::unix::fs::PermissionsExt;
            Some(metadata.permissions().mode())
        };
        #[cfg(not(unix))]
        let file_mode = None;

        if metadata.len() > MAX_BACKUP_FILE_SIZE {
            tracing::warn!(
                path = %abs_path.display(),
                size = metadata.len(),
                "Skipping backup: file exceeds {MAX_BACKUP_FILE_SIZE} byte limit"
            );
            return Ok(());
        }

        // Read and hash content
        let content = tokio::fs::read(&abs_path)
            .await
            .context(file_backup_error::IoSnafu {
                message: "reading file content for backup".to_string(),
            })?;
        let content_hash = hex_sha256(&content);

        let mut idx = self.index.lock().await;

        // Content dedup: check if we already have this exact content
        let backup_filename = if let Some(existing) = idx.content_map.get(&content_hash) {
            existing.clone()
        } else {
            // Generate unique filename
            let path_hash = path_hash16(&abs_path);
            let version = idx.next_version.entry(path_hash.clone()).or_insert(0);
            *version += 1;
            let filename = format!("{path_hash}@v{version}");

            // Write backup blob
            let blob_path = self.backup_dir.join(&filename);
            tokio::fs::write(&blob_path, &content)
                .await
                .context(file_backup_error::IoSnafu {
                    message: "writing backup blob".to_string(),
                })?;

            idx.content_map
                .insert(content_hash.clone(), filename.clone());
            filename
        };

        let entry = BackupEntry {
            original_path: abs_path,
            content_hash,
            backup_filename,
            existed_before: true,
            turn_id: turn_id.clone(),
            file_mode,
        };
        idx.turns.entry(turn_id).or_default().push(entry);
        self.save_index_locked(&idx).await;

        Ok(())
    }

    /// Restore all files modified during the given turn to their pre-modification state.
    ///
    /// Uses multi-tier comparison to skip files that already match their backup state:
    /// 1. Existence check — detect created/deleted files
    /// 2. Size comparison — different sizes guarantee different content
    /// 3. Content hash comparison — definitive check using stored SHA256
    pub async fn restore_turn(&self, turn_id: &str) -> Result<Vec<PathBuf>> {
        let idx = self.index.lock().await;
        let entries = match idx.turns.get(turn_id) {
            Some(e) => e.clone(),
            None => return Ok(Vec::new()),
        };
        drop(idx);

        let mut restored = Vec::new();
        for entry in &entries {
            if entry.existed_before {
                // Restore from backup blob
                let blob_path = self.backup_dir.join(&entry.backup_filename);
                if !tokio::fs::try_exists(&blob_path).await.unwrap_or(false) {
                    tracing::warn!(
                        path = %entry.original_path.display(),
                        blob = %entry.backup_filename,
                        "Backup blob missing, cannot restore file"
                    );
                    continue;
                }

                // Multi-tier check: skip restore if file already matches backup.
                if !Self::file_needs_restore(&entry.original_path, entry, &self.backup_dir).await {
                    continue;
                }

                let blob_content =
                    tokio::fs::read(&blob_path)
                        .await
                        .context(file_backup_error::IoSnafu {
                            message: "reading backup blob".to_string(),
                        })?;
                tokio::fs::write(&entry.original_path, blob_content)
                    .await
                    .context(file_backup_error::IoSnafu {
                        message: "restoring file from backup".to_string(),
                    })?;
                // Restore file permissions if stored
                #[cfg(unix)]
                if let Some(mode) = entry.file_mode {
                    use std::os::unix::fs::PermissionsExt;
                    let perms = std::fs::Permissions::from_mode(mode);
                    tokio::fs::set_permissions(&entry.original_path, perms)
                        .await
                        .ok();
                }
                restored.push(entry.original_path.clone());
            } else {
                // File was newly created - delete it
                if tokio::fs::try_exists(&entry.original_path)
                    .await
                    .unwrap_or(false)
                {
                    tokio::fs::remove_file(&entry.original_path).await.ok();
                    restored.push(entry.original_path.clone());
                }
            }
        }
        Ok(restored)
    }

    /// Restore a single backup entry to its original path.
    ///
    /// Returns `true` if the file was actually modified on disk.
    /// Uses the same multi-tier comparison as `restore_turn`.
    pub async fn restore_entry(&self, entry: &BackupEntry) -> Result<bool> {
        if entry.existed_before {
            let blob_path = self.backup_dir.join(&entry.backup_filename);
            if !tokio::fs::try_exists(&blob_path).await.unwrap_or(false) {
                tracing::warn!(
                    path = %entry.original_path.display(),
                    blob = %entry.backup_filename,
                    "Backup blob missing, cannot restore file"
                );
                return Ok(false);
            }

            if !Self::file_needs_restore(&entry.original_path, entry, &self.backup_dir).await {
                return Ok(false);
            }

            let blob_content =
                tokio::fs::read(&blob_path)
                    .await
                    .context(file_backup_error::IoSnafu {
                        message: "reading backup blob".to_string(),
                    })?;
            tokio::fs::write(&entry.original_path, blob_content)
                .await
                .context(file_backup_error::IoSnafu {
                    message: "restoring file from backup".to_string(),
                })?;
            #[cfg(unix)]
            if let Some(mode) = entry.file_mode {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(mode);
                tokio::fs::set_permissions(&entry.original_path, perms)
                    .await
                    .ok();
            }
            Ok(true)
        } else {
            // File was newly created — delete it.
            if tokio::fs::try_exists(&entry.original_path)
                .await
                .unwrap_or(false)
            {
                tokio::fs::remove_file(&entry.original_path).await.ok();
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }

    /// Multi-tier check whether a file needs restoration from backup.
    ///
    /// Tiers (cheapest first):
    /// 1. Existence — if file doesn't exist on disk, it needs restore.
    /// 2. Size — if sizes differ, content must differ.
    /// 3. Content hash — definitive SHA256 comparison.
    async fn file_needs_restore(original: &Path, entry: &BackupEntry, backup_dir: &Path) -> bool {
        // Tier 1: existence
        let Ok(orig_meta) = tokio::fs::metadata(original).await else {
            return true; // File doesn't exist — needs restore
        };

        // Tier 2: size comparison
        let blob_path = backup_dir.join(&entry.backup_filename);
        let Ok(backup_meta) = tokio::fs::metadata(&blob_path).await else {
            return true; // Can't stat backup — assume needs restore
        };
        if orig_meta.len() != backup_meta.len() {
            return true;
        }

        // Tier 3: content hash comparison (avoids reading both files when hash matches)
        if !entry.content_hash.is_empty() {
            let Ok(content) = tokio::fs::read(original).await else {
                return true;
            };
            let current_hash = hex_sha256(&content);
            return current_hash != entry.content_hash;
        }

        // No stored hash — conservative: assume needs restore
        true
    }

    /// Get backup entries for a specific turn.
    pub async fn entries_for_turn(&self, turn_id: &str) -> Vec<BackupEntry> {
        let idx = self.index.lock().await;
        idx.turns.get(turn_id).cloned().unwrap_or_default()
    }

    /// Remove backup entries for a specific turn (after successful restore or cleanup).
    ///
    /// Also cleans up orphaned content_map entries and blob files that are
    /// no longer referenced by any remaining turn.
    pub async fn remove_turn(&self, turn_id: &str) {
        let mut idx = self.index.lock().await;
        let removed_entries = idx.turns.remove(turn_id).unwrap_or_default();

        // Collect content hashes still referenced by remaining turns.
        let still_used: HashSet<String> = idx
            .turns
            .values()
            .flatten()
            .filter(|e| e.existed_before)
            .map(|e| e.content_hash.clone())
            .collect();

        // Identify orphaned content hashes from the removed entries.
        let orphaned_hashes: Vec<String> = removed_entries
            .iter()
            .filter(|e| e.existed_before && !e.content_hash.is_empty())
            .map(|e| e.content_hash.clone())
            .filter(|h| !still_used.contains(h))
            .collect();

        // Remove orphaned content_map entries and their blob files.
        for hash in &orphaned_hashes {
            if let Some(blob_name) = idx.content_map.remove(hash) {
                let blob_path = self.backup_dir.join(&blob_name);
                if let Err(e) = tokio::fs::remove_file(&blob_path).await
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    tracing::warn!(blob = %blob_name, "Failed to remove backup blob: {e}");
                }
            }
        }

        self.save_index_locked(&idx).await;
    }

    /// Get the backup directory path (used by SnapshotManager for dry-run diff).
    pub fn backup_dir(&self) -> &Path {
        &self.backup_dir
    }

    /// Persist the index to disk.
    async fn save_index_locked(&self, idx: &BackupIndex) {
        let index_path = self.backup_dir.join("index.json");
        match serde_json::to_string_pretty(idx) {
            Ok(json) => {
                if let Err(e) = tokio::fs::write(&index_path, json).await {
                    tracing::warn!("Failed to persist backup index: {e}");
                }
            }
            Err(e) => {
                tracing::warn!("Failed to serialize backup index: {e}");
            }
        }
    }

    /// Load the index from disk, or return a default.
    async fn load_index(backup_dir: &Path) -> BackupIndex {
        let index_path = backup_dir.join("index.json");
        match tokio::fs::read_to_string(&index_path).await {
            Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
            Err(_) => BackupIndex::default(),
        }
    }
}

/// Compute SHA256 hex digest.
fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Compute a 16-char hex prefix from a path for use as filename prefix.
fn path_hash16(path: &Path) -> String {
    let full = hex_sha256(path.to_string_lossy().as_bytes());
    full[..16].to_string()
}

#[cfg(test)]
#[path = "backup.test.rs"]
mod tests;
