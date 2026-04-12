//! Session-level file-read state cache.
//!
//! TS: `readFileState` / `FileStateCache` — LRU cache (100 entries, 25MB) tracking
//! all files read by tools or @mentions with `{content, mtime, offset, limit}`.
//!
//! Enables:
//! - @mention deduplication (already-read check via mtime comparison)
//! - Edit safety (reject if file modified externally since last read)
//! - Changed-file detection between turns (mtime comparison)
//!
//! **Different from `FileReadCache`** which is a simple same-turn LRU optimization
//! with no mtime tracking and no tool integration.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

/// Maximum cached entries (matches TS `FileStateCache` max size).
const MAX_ENTRIES: usize = 100;

/// A file entry in the read state cache.
#[derive(Debug, Clone)]
pub struct FileReadEntry {
    /// File content at the time of the last read.
    pub content: String,
    /// File modification time (epoch ms) when last read.
    pub mtime_ms: i64,
    /// Line offset if this was a partial read.
    pub offset: Option<i32>,
    /// Line limit if this was a partial read.
    pub limit: Option<i32>,
}

/// Session-level cache of file read states.
///
/// Tracks every file read by tools (`Read`, `Edit`, `Write`) and @mentions
/// with their content and modification time for deduplication and change detection.
#[derive(Debug, Default)]
pub struct FileReadState {
    entries: HashMap<PathBuf, FileReadEntry>,
    /// LRU ordering (most-recently-accessed at end).
    access_order: Vec<PathBuf>,
}

impl FileReadState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&mut self, path: &Path) -> Option<&FileReadEntry> {
        let canonical = path.to_path_buf();
        if self.entries.contains_key(&canonical) {
            self.touch_lru(&canonical);
            self.entries.get(&canonical)
        } else {
            None
        }
    }

    pub fn peek(&self, path: &Path) -> Option<&FileReadEntry> {
        self.entries.get(&path.to_path_buf())
    }

    /// Record a file read (from tool or @mention).
    pub fn set(&mut self, path: PathBuf, entry: FileReadEntry) {
        let canonical = path.to_path_buf();
        self.evict_if_full();
        self.touch_lru(&canonical);
        self.entries.insert(canonical, entry);
    }

    /// Update after an edit/write: new content, new mtime, clear partial-read markers.
    ///
    /// TS: `FileEditTool` line 520-525 — clears offset/limit after write.
    pub fn update_after_edit(&mut self, path: &Path, new_content: String, new_mtime_ms: i64) {
        let canonical = path.to_path_buf();
        self.touch_lru(&canonical);
        self.entries.insert(
            canonical,
            FileReadEntry {
                content: new_content,
                mtime_ms: new_mtime_ms,
                offset: None,
                limit: None,
            },
        );
    }

    /// Returns `true` if the file is in cache and disk mtime matches.
    pub async fn is_unchanged(&self, path: &Path) -> bool {
        let canonical = path.to_path_buf();
        let Some(entry) = self.entries.get(&canonical) else {
            return false;
        };
        match file_mtime_ms(&canonical).await {
            Ok(disk_mtime) => entry.mtime_ms == disk_mtime,
            Err(_) => false,
        }
    }

    pub fn invalidate(&mut self, path: &Path) {
        let canonical = path.to_path_buf();
        self.entries.remove(&canonical);
        self.access_order.retain(|p| p != &canonical);
    }

    /// Iterate all cached entries (for changed-file detection).
    pub fn iter_entries(&self) -> impl Iterator<Item = (&Path, &FileReadEntry)> {
        self.entries.iter().map(|(k, v)| (k.as_path(), v))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.access_order.clear();
    }

    /// Snapshot all entries ordered by access recency (most recent last).
    ///
    /// TS: `cacheToObject(context.readFileState)` — captures pre-compact state.
    /// Used by compact to snapshot before clearing, so post-compact file
    /// restoration can re-inject the most recently accessed files.
    pub fn snapshot_by_recency(&self) -> Vec<(PathBuf, FileReadEntry)> {
        self.access_order
            .iter()
            .filter_map(|path| {
                self.entries
                    .get(path)
                    .map(|entry| (path.clone(), entry.clone()))
            })
            .collect()
    }

    // -- Internal helpers --

    fn touch_lru(&mut self, path: &PathBuf) {
        self.access_order.retain(|p| p != path);
        self.access_order.push(path.clone());
    }

    fn evict_if_full(&mut self) {
        while self.entries.len() >= MAX_ENTRIES {
            if let Some(oldest) = self.access_order.first().cloned() {
                self.entries.remove(&oldest);
                self.access_order.remove(0);
            } else {
                break;
            }
        }
    }
}

/// Get file modification time in epoch milliseconds.
pub async fn file_mtime_ms(path: &Path) -> std::io::Result<i64> {
    let meta = tokio::fs::metadata(path).await?;
    let mtime = meta
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Ok(mtime)
}

#[cfg(test)]
#[path = "file_read_state.test.rs"]
mod tests;
