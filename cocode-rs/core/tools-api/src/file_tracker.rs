//! File read/modification tracking with LRU eviction.
//!
//! Provides [`FileTracker`] for tracking file reads and modifications,
//! with LRU eviction, change detection, and content hashing.
//! [`FileReadState`] captures per-file read metadata for already-read detection.

pub use cocode_protocol::FileReadKind;

use lru::LruCache;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::collections::HashMap;
use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

///
/// # Claude Code Alignment
///
/// Uses `i64` for offset/limit to support large files (>2 billion lines).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileReadState {
    /// File content at time of read (None if partial or too large).
    pub content: Option<String>,
    /// When this read state was recorded (not serialized - in-memory only).
    #[serde(skip)]
    pub timestamp: SystemTime,
    /// File modification time at time of read.
    pub file_mtime: Option<SystemTime>,
    /// SHA256 hex hash of content at time of read (None if partial or too large).
    pub content_hash: Option<String>,
    /// Line offset of the read (None if from start).
    /// Uses i64 for large file support (>2 billion lines).
    pub offset: Option<i64>,
    /// Line limit of the read (None if no limit).
    /// Uses i64 for large file support (>2 billion lines).
    pub limit: Option<i64>,
    /// Kind of read operation.
    pub kind: FileReadKind,
    /// Number of times this file has been accessed.
    pub access_count: i32,
    /// Turn number when the file was read (for compaction cleanup).
    pub read_turn: i32,
}

impl Default for FileReadState {
    fn default() -> Self {
        Self {
            content: None,
            timestamp: SystemTime::UNIX_EPOCH,
            file_mtime: None,
            content_hash: None,
            offset: None,
            limit: None,
            kind: FileReadKind::MetadataOnly,
            access_count: 0,
            read_turn: 0,
        }
    }
}

impl FileReadState {
    /// Compute SHA256 hex hash of content.
    pub fn compute_hash(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Create a new read state for a complete file read.
    pub fn complete(content: String, file_mtime: Option<SystemTime>) -> Self {
        Self::complete_with_turn(content, file_mtime, 0)
    }

    /// Create a new read state for a complete file read with turn number.
    pub fn complete_with_turn(
        content: String,
        file_mtime: Option<SystemTime>,
        read_turn: i32,
    ) -> Self {
        let hash = Self::compute_hash(&content);
        Self {
            content: Some(content),
            timestamp: SystemTime::now(),
            file_mtime,
            content_hash: Some(hash),
            offset: None,
            limit: None,
            kind: FileReadKind::FullContent,
            access_count: 1,
            read_turn,
        }
    }

    /// Create a new read state for a partial file read.
    pub fn partial(offset: i64, limit: i64, file_mtime: Option<SystemTime>) -> Self {
        Self::partial_with_turn(offset, limit, file_mtime, 0)
    }

    /// Create a new read state for a partial file read with turn number.
    pub fn partial_with_turn(
        offset: i64,
        limit: i64,
        file_mtime: Option<SystemTime>,
        read_turn: i32,
    ) -> Self {
        Self {
            content: None,
            timestamp: SystemTime::now(),
            file_mtime,
            content_hash: None,
            offset: Some(offset),
            limit: Some(limit),
            kind: FileReadKind::PartialContent,
            access_count: 1,
            read_turn,
        }
    }

    /// Create a new read state for metadata-only (path discovery).
    pub fn metadata_only(file_mtime: Option<SystemTime>, read_turn: i32) -> Self {
        Self {
            content: None,
            timestamp: SystemTime::now(),
            file_mtime,
            content_hash: None,
            offset: None,
            limit: None,
            kind: FileReadKind::MetadataOnly,
            access_count: 1,
            read_turn,
        }
    }

    /// Create a read state with content and full metadata.
    ///
    /// This is the primary constructor for system-reminder compatibility,
    /// used when rebuilding state from ContextModifier::FileRead.
    ///
    /// # Arguments
    ///
    /// * `content` - File content
    /// * `last_modified` - File modification time at read time
    /// * `read_turn` - Turn number when the file was read
    /// * `offset` - Line offset (0 if from start)
    /// * `limit` - Line limit (0 if no limit)
    ///
    /// # Claude Code Alignment
    ///
    /// This matches the constructor pattern used in Claude Code v2.1.38's
    /// file read state reconstruction.
    pub fn with_content(
        content: String,
        last_modified: Option<SystemTime>,
        read_turn: i32,
        offset: i64,
        limit: i64,
    ) -> Self {
        let has_content = !content.is_empty();
        let is_full = offset == 0 && limit == 0;
        let content_hash = if has_content {
            Some(Self::compute_hash(&content))
        } else {
            None
        };

        Self {
            content: if has_content { Some(content) } else { None },
            timestamp: SystemTime::now(),
            file_mtime: last_modified,
            content_hash,
            offset: if offset > 0 { Some(offset) } else { None },
            limit: if limit > 0 { Some(limit) } else { None },
            kind: if is_full {
                FileReadKind::FullContent
            } else {
                FileReadKind::PartialContent
            },
            access_count: 1,
            read_turn,
        }
    }

    /// Return a normalized copy of this state.
    ///
    /// Normalization ensures consistency between the `kind` field and
    /// the `offset`/`limit`/`content_hash` fields:
    ///
    /// - `FullContent`: offset/limit are cleared, content_hash preserved
    /// - `PartialContent`: content_hash is cleared (partial reads can't verify)
    /// - `MetadataOnly`: content cleared, offset/limit set to None
    ///
    /// # Claude Code Alignment
    ///
    /// This matches Claude Code v2.1.38's state normalization behavior.
    pub fn normalized(mut self) -> Self {
        self.normalize_in_place();
        self
    }

    /// Normalize this state in place.
    fn normalize_in_place(&mut self) {
        match self.kind {
            FileReadKind::FullContent => {
                // Full reads should not have offset/limit
                self.offset = None;
                self.limit = None;
            }
            FileReadKind::PartialContent => {
                // Partial reads should not have content hash
                // (can't verify content hasn't changed)
                self.content_hash = None;
            }
            FileReadKind::MetadataOnly => {
                // Metadata-only has no content or range
                self.content = None;
                self.content_hash = None;
                self.offset = None;
                self.limit = None;
            }
        }
    }

    /// Check if this was a full content read.
    pub fn is_full(&self) -> bool {
        self.kind.is_full()
    }

    /// Check if this was a partial read.
    pub fn is_partial(&self) -> bool {
        self.kind.is_partial()
    }

    /// Check if this was a metadata-only read.
    pub fn is_metadata_only(&self) -> bool {
        self.kind.is_metadata_only()
    }

    /// Check if this is a cacheable read (full content only).
    /// Used for already-read detection.
    pub fn is_cacheable(&self) -> bool {
        matches!(self.kind, FileReadKind::FullContent)
    }
}

/// Configuration for FileTracker limits.
///
/// Provides clear, named configuration for the file tracker's LRU cache
/// behavior. This matches Claude Code v2.1.38's limits.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FileTrackerConfig {
    /// Maximum number of entries in the LRU cache.
    ///
    /// When this limit is reached, the oldest (least recently used) entries
    /// are evicted to make room for new ones.
    ///
    /// Default: 100 (Claude Code v2.1.38 alignment)
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,

    /// Maximum total content size in bytes across all tracked files.
    ///
    /// When this limit is approached, older entries are evicted to stay
    /// under the budget. This prevents unbounded memory growth.
    ///
    /// Default: ~25MB (26,214,400 bytes - Claude Code v2.1.38 alignment)
    #[serde(default = "default_max_size_bytes")]
    pub max_total_bytes: usize,
}

fn default_max_entries() -> usize {
    100
}

fn default_max_size_bytes() -> usize {
    26_214_400 // ~25MB
}

impl Default for FileTrackerConfig {
    fn default() -> Self {
        Self {
            max_entries: default_max_entries(),
            max_total_bytes: default_max_size_bytes(),
        }
    }
}

impl FileTrackerConfig {
    /// Create a new config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a config with custom limits.
    pub fn with_limits(max_entries: usize, max_total_bytes: usize) -> Self {
        Self {
            max_entries,
            max_total_bytes,
        }
    }
}

/// Internal state for FileTracker.
///
/// Separated from the outer struct to enable RwLock-based interior mutability.
#[derive(Debug)]
struct TrackerState {
    /// Files that have been read, with their read state (LRU cache).
    read_files: LruCache<PathBuf, FileReadState>,
    /// Files that have been modified.
    modified_files: HashSet<PathBuf>,
    /// Paths that trigger nested memory lookup (CLAUDE.md, AGENTS.md, etc.).
    nested_memory_triggers: HashSet<PathBuf>,
    /// Mapping from tool call IDs to file paths (for cleanup during compaction).
    tool_id_to_path: HashMap<String, PathBuf>,
    /// Current total content size in bytes.
    current_size_bytes: usize,
}

/// Tracks files that have been read or modified.
///
/// This is the unified file tracker for the agent system, handling:
/// - Read state tracking (content, mtime, access patterns)
/// - Modification tracking
/// - Change detection (comparing current mtime to read-time mtime)
/// - Nested memory triggers (CLAUDE.md, AGENTS.md, etc.)
/// - Tool call ID to file path mapping (for compaction cleanup)
/// - LRU eviction with size limits
///
/// # Interior Mutability
///
/// Uses `RwLock` internally to allow shared access (`&self`) for all operations.
/// This enables:
/// - Concurrent reads via `read()` guard
/// - Exclusive writes via `write()` guard
/// - Snapshot generation without blocking writes for long
///
/// # LRU Eviction
///
/// The tracker uses an LRU cache with configurable limits:
/// - Maximum 100 entries (configurable via `with_limits`)
/// - Maximum ~25MB total content size (configurable via `with_max_size_bytes`)
///
/// When limits are exceeded, oldest entries are evicted automatically.
#[derive(Debug)]
pub struct FileTracker {
    /// Internal state protected by RwLock for interior mutability.
    state: std::sync::RwLock<TrackerState>,
    /// Maximum total content size in bytes (default: ~25MB).
    max_size_bytes: usize,
}

impl Default for FileTracker {
    fn default() -> Self {
        Self::with_config(FileTrackerConfig::default())
    }
}

impl FileTracker {
    /// Acquire a read guard, recovering from lock poisoning.
    fn read_guard(&self) -> std::sync::RwLockReadGuard<'_, TrackerState> {
        self.state.read().unwrap_or_else(|e| {
            tracing::warn!("Lock poisoned — concurrent bug detected");
            e.into_inner()
        })
    }

    /// Acquire a write guard, recovering from lock poisoning.
    fn write_guard(&self) -> std::sync::RwLockWriteGuard<'_, TrackerState> {
        self.state.write().unwrap_or_else(|e| {
            tracing::warn!("Lock poisoned — concurrent bug detected");
            e.into_inner()
        })
    }

    /// Create a new file tracker with default limits (100 entries, ~25MB).
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a file tracker with a configuration.
    ///
    /// This is the preferred constructor for explicit control over limits.
    pub fn with_config(config: FileTrackerConfig) -> Self {
        Self::with_limits(config.max_entries, config.max_total_bytes)
    }

    /// Create a file tracker with custom limits.
    ///
    /// # Arguments
    /// * `max_entries` - Maximum number of files to track (sets LRU capacity)
    /// * `max_size_bytes` - Maximum total content size in bytes
    pub fn with_limits(max_entries: usize, max_size_bytes: usize) -> Self {
        // SAFETY: max(1) guarantees the value is at least 1
        let capacity = NonZeroUsize::new(max_entries.max(1)).unwrap_or(NonZeroUsize::MIN);
        Self {
            state: std::sync::RwLock::new(TrackerState {
                read_files: LruCache::new(capacity),
                modified_files: HashSet::new(),
                nested_memory_triggers: HashSet::new(),
                tool_id_to_path: HashMap::new(),
                current_size_bytes: 0,
            }),
            max_size_bytes,
        }
    }

    /// Create a file tracker with capacity (for pre-allocation).
    pub fn with_capacity(capacity: usize) -> Self {
        Self::with_limits(capacity, 26_214_400)
    }

    /// Get the current number of tracked files.
    pub fn len(&self) -> usize {
        self.read_guard().read_files.len()
    }

    /// Check if the tracker is empty.
    pub fn is_empty(&self) -> bool {
        self.read_guard().read_files.is_empty()
    }

    /// Get current total content size in bytes.
    pub fn current_size(&self) -> usize {
        self.read_guard().current_size_bytes
    }

    /// Get all read files with their state for syncing to another tracker.
    ///
    /// This is used to sync file read state to the system-reminder's FileTracker
    /// for change detection.
    ///
    /// Returns owned data (cloned) to avoid holding the read lock.
    pub fn read_files_with_state(&self) -> Vec<(PathBuf, FileReadState)> {
        let state = self.read_guard();
        state
            .read_files
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Record a file read (simple — backward-compatible).
    pub fn record_read(&self, path: impl Into<PathBuf>) {
        let path = path.into();
        // Skip internal files (session memory, plan files, etc.)
        if Self::is_internal_file(&path) {
            return;
        }
        let mut state = self.write_guard();
        if let Some(read_state) = state.read_files.get_mut(&path) {
            read_state.access_count += 1;
            read_state.timestamp = SystemTime::now();
        } else {
            drop(state); // Release write lock before re-acquiring with eviction
            self.insert_with_eviction(
                path,
                FileReadState {
                    content: None,
                    timestamp: SystemTime::now(),
                    file_mtime: None,
                    content_hash: None,
                    offset: None,
                    limit: None,
                    kind: FileReadKind::MetadataOnly,
                    access_count: 1,
                    read_turn: 0,
                },
            );
        }
    }

    /// Record a file read with full state.
    pub fn record_read_with_state(&self, path: impl Into<PathBuf>, read_state: FileReadState) {
        let path = path.into();
        // Skip internal files (session memory, plan files, etc.)
        if Self::is_internal_file(&path) {
            return;
        }
        self.insert_with_eviction(path, read_state);
    }

    /// Insert a file with state, handling LRU eviction.
    fn insert_with_eviction(&self, path: PathBuf, read_state: FileReadState) {
        let content_size = read_state.content.as_ref().map(String::len).unwrap_or(0);
        let max_size = self.max_size_bytes;

        let mut state = self.write_guard();

        // Check if we need to evict entries for size
        while state.current_size_bytes + content_size > max_size && !state.read_files.is_empty() {
            // Evict oldest entry
            if let Some((_, old_state)) = state.read_files.pop_lru() {
                let old_size = old_state.content.as_ref().map(String::len).unwrap_or(0);
                state.current_size_bytes = state.current_size_bytes.saturating_sub(old_size);
            }
        }

        // If this path already exists, update the size accounting
        if let Some(old_state) = state.read_files.peek(&path) {
            let old_size = old_state.content.as_ref().map(String::len).unwrap_or(0);
            state.current_size_bytes = state.current_size_bytes.saturating_sub(old_size);
        }

        // Add the new size
        state.current_size_bytes += content_size;

        // Insert the entry
        state.read_files.put(path, read_state);
    }

    /// Record a file modification.
    pub fn record_modified(&self, path: impl Into<PathBuf>) {
        let mut state = self.write_guard();
        state.modified_files.insert(path.into());
    }

    /// Check if a file has been read.
    pub fn was_read(&self, path: &Path) -> bool {
        let state = self.read_guard();
        state.read_files.contains(path)
    }

    /// Get the read state for a file (cloned to avoid holding lock).
    pub fn read_state(&self, path: &Path) -> Option<FileReadState> {
        let state = self.read_guard();
        state.read_files.peek(path).cloned()
    }

    /// Check if a file has been modified.
    pub fn was_modified(&self, path: &Path) -> bool {
        let state = self.read_guard();
        state.modified_files.contains(path)
    }

    /// Get all read file paths.
    pub fn read_files(&self) -> Vec<PathBuf> {
        let state = self.read_guard();
        state.read_files.iter().map(|(k, _)| k.clone()).collect()
    }

    /// Get all modified files.
    pub fn modified_files(&self) -> HashSet<PathBuf> {
        let state = self.read_guard();
        state.modified_files.clone()
    }

    /// Track a file read with full state.
    ///
    /// Returns `true` if this file triggers nested memory lookup
    /// (e.g., CLAUDE.md, AGENTS.md files).
    pub fn track_read(&self, path: impl Into<PathBuf>, read_state: FileReadState) -> bool {
        let path = path.into();
        let is_memory_trigger = Self::is_nested_memory_trigger(&path);

        self.insert_with_eviction(path.clone(), read_state);

        if is_memory_trigger {
            let mut state = self.state.write().unwrap_or_else(|e| {
                tracing::warn!("Lock poisoned — concurrent bug detected");
                e.into_inner()
            });
            state.nested_memory_triggers.insert(path);
            true
        } else {
            false
        }
    }

    /// Check if a file has changed since it was last read.
    ///
    /// Returns `None` if the file isn't tracked.
    /// Skips change detection for partial reads.
    ///
    /// A file is considered changed if its current mtime differs from the stored mtime.
    /// This uses exact comparison (not just `new > old`) to detect any modification.
    pub fn has_file_changed(&self, path: &Path) -> Option<bool> {
        // Get state under read lock, then release lock before filesystem access
        let (file_mtime, content_hash, is_partial) = {
            let state = self.state.read().unwrap_or_else(|e| {
                tracing::warn!("Lock poisoned — concurrent bug detected");
                e.into_inner()
            });
            let read_state = state.read_files.peek(path)?;
            let is_partial = read_state.is_partial();
            (
                read_state.file_mtime,
                read_state.content_hash.clone(),
                is_partial,
            )
        };

        // Skip partial reads - can't reliably detect changes
        if is_partial {
            return Some(false);
        }

        // Check modification time - exact match means unchanged
        let current_mtime = std::fs::metadata(path).ok()?.modified().ok();

        match (file_mtime, current_mtime) {
            (Some(old), Some(new)) => Some(new != old), // Changed if mtime differs
            (None, Some(_)) => Some(true),              // File now has mtime
            (Some(_), None) => Some(true),              // File lost mtime (weird but changed)
            (None, None) => {
                // Fall back to content comparison
                let current_content = std::fs::read_to_string(path).ok()?;
                let current_hash = FileReadState::compute_hash(&current_content);
                Some(Some(&current_hash) != content_hash.as_ref())
            }
        }
    }

    /// Check if a file is unchanged since it was last read.
    ///
    /// Returns `None` if the file isn't tracked or can't be checked.
    /// Returns `Some(true)` if the file's current mtime exactly matches the stored mtime.
    /// This is used for already-read-files detection to skip re-reading unchanged files.
    ///
    /// # Claude Code Alignment
    ///
    /// This matches Claude Code v2.1.38's behavior: an exact mtime match indicates
    /// the file hasn't been modified since it was read. This is more precise than
    /// just checking `new > old` because it catches any change, not just newer.
    pub fn is_unchanged(&self, path: &Path) -> Option<bool> {
        // Get state under read lock, then release lock before filesystem access
        let (file_mtime, content_hash, is_partial) = {
            let state = self.state.read().unwrap_or_else(|e| {
                tracing::warn!("Lock poisoned — concurrent bug detected");
                e.into_inner()
            });
            let read_state = state.read_files.peek(path)?;
            let is_partial = read_state.is_partial();
            (
                read_state.file_mtime,
                read_state.content_hash.clone(),
                is_partial,
            )
        };

        // Partial reads are NOT cacheable - return None
        // This ensures @mentioned files with partial reads are always re-read
        if is_partial {
            return None;
        }

        // Check modification time - exact match means unchanged
        let current_mtime = std::fs::metadata(path).ok()?.modified().ok();

        match (file_mtime, current_mtime) {
            (Some(old), Some(new)) => Some(new == old), // Unchanged only if exact match
            (None, None) => {
                // No mtime available, fall back to content hash comparison
                let current_content = std::fs::read_to_string(path).ok()?;
                let current_hash = FileReadState::compute_hash(&current_content);
                Some(Some(&current_hash) == content_hash.as_ref())
            }
            // If we had no mtime before but do now, or vice versa, consider it potentially changed
            _ => None,
        }
    }

    /// Get all tracked file paths.
    pub fn tracked_files(&self) -> Vec<PathBuf> {
        let state = self.read_guard();
        state.read_files.iter().map(|(k, _)| k.clone()).collect()
    }

    /// Get files that have changed since last read.
    pub fn changed_files(&self) -> Vec<PathBuf> {
        self.tracked_files()
            .into_iter()
            .filter(|p| self.has_file_changed(p) == Some(true))
            .collect()
    }

    /// Update the modification time for a file after editing.
    pub fn update_modified_time(&self, path: &Path) {
        let mut state = self.write_guard();
        if let Some(read_state) = state.read_files.get_mut(path)
            && let Ok(meta) = std::fs::metadata(path)
        {
            read_state.file_mtime = meta.modified().ok();
        }
    }

    /// Remove tracking for a file.
    pub fn remove(&self, path: &Path) {
        let mut state = self.write_guard();
        if let Some(read_state) = state.read_files.pop(path) {
            let size = read_state.content.as_ref().map(String::len).unwrap_or(0);
            state.current_size_bytes = state.current_size_bytes.saturating_sub(size);
        }
        state.nested_memory_triggers.remove(path);
    }

    /// Enforce an entry limit by evicting the least-recently-used entries.
    ///
    /// Pops LRU entries until the count is at most `max_entries`.
    pub fn enforce_entry_limit(&self, max_entries: usize) {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while state.read_files.len() > max_entries {
            if let Some((_path, evicted)) = state.read_files.pop_lru() {
                if let Some(content) = &evicted.content {
                    state.current_size_bytes =
                        state.current_size_bytes.saturating_sub(content.len());
                }
            } else {
                break;
            }
        }
    }

    /// Clear all tracked files.
    pub fn clear(&self) {
        let mut state = self.write_guard();
        state.read_files.clear();
        state.modified_files.clear();
        state.nested_memory_triggers.clear();
        state.current_size_bytes = 0;
    }

    /// Get and clear nested memory trigger paths.
    ///
    /// Returns paths that need nested memory lookup, then clears them.
    pub fn drain_nested_memory_triggers(&self) -> HashSet<PathBuf> {
        let mut state = self.write_guard();
        std::mem::take(&mut state.nested_memory_triggers)
    }

    /// Check if there are pending nested memory triggers.
    pub fn has_nested_memory_triggers(&self) -> bool {
        let state = self.read_guard();
        !state.nested_memory_triggers.is_empty()
    }

    /// Check if a path triggers nested memory lookup.
    fn is_nested_memory_trigger(path: &Path) -> bool {
        let filename = path.file_name().and_then(|n| n.to_str());
        matches!(
            filename,
            Some("CLAUDE.md" | "AGENTS.md" | "settings.json" | ".cursorrules" | ".aider.conf.yml")
        )
    }

    /// Check if a path is an internal file that shouldn't be tracked for compaction.
    ///
    /// Internal files include session memory files, plan files, and other system files
    /// that shouldn't be preserved during compaction restoration.
    fn is_internal_file(path: &Path) -> bool {
        let path_str = path.to_string_lossy();

        // Session memory file
        if path_str.contains("session-memory") && path_str.contains("summary.md") {
            return true;
        }

        // Plan files (in ~/.cocode/plans/)
        if path_str.contains(".cocode/plans/") {
            return true;
        }

        // Auto memory files (MEMORY.md or project memory)
        if let Some(filename) = path.file_name().and_then(|n| n.to_str())
            && (filename == "MEMORY.md" || filename.starts_with("memory-"))
        {
            return true;
        }

        // Tool result persistence files
        if path_str.contains("tool-results/") {
            return true;
        }

        false
    }

    /// Register a file read with its tool call ID for compaction cleanup.
    ///
    /// When micro-compact removes tool results, this mapping allows
    /// cleaning up the corresponding FileTracker entries.
    pub fn register_tool_read(&self, tool_call_id: String, path: PathBuf) {
        let mut state = self.write_guard();
        state.tool_id_to_path.insert(tool_call_id, path);
    }

    /// Clean up file tracker entries for compacted tool call IDs.
    ///
    /// Called after micro-compaction to remove entries for compacted reads.
    pub fn cleanup_compacted(&self, compacted_ids: &[String]) {
        let mut state = self.write_guard();
        for id in compacted_ids {
            if let Some(path) = state.tool_id_to_path.remove(id) {
                // Remove from read_files if present
                if let Some(read_state) = state.read_files.pop(&path) {
                    let size = read_state.content.as_ref().map(String::len).unwrap_or(0);
                    state.current_size_bytes = state.current_size_bytes.saturating_sub(size);
                }
                state.nested_memory_triggers.remove(&path);
            }
        }
    }

    /// Get the mapping of tool call IDs to paths (for testing/debugging).
    pub fn tool_id_paths(&self) -> HashMap<String, PathBuf> {
        let state = self.read_guard();
        state.tool_id_to_path.clone()
    }

    /// Get the most recent files sorted by timestamp (for compaction restoration).
    ///
    /// Returns up to `limit` file paths sorted by most recent access.
    pub fn most_recent_files(&self, limit: usize) -> Vec<PathBuf> {
        let state = self.read_guard();
        let mut files: Vec<_> = state
            .read_files
            .iter()
            .filter(|(_, read_state)| read_state.content.is_some())
            .collect::<Vec<_>>();

        // Sort by timestamp (most recent first)
        files.sort_by(|a, b| {
            b.1.timestamp
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .cmp(
                    &a.1.timestamp
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default(),
                )
        });

        files
            .into_iter()
            .take(limit)
            .map(|(p, _)| p.clone())
            .collect()
    }

    /// Check if a file has been fully read and is unchanged.
    ///
    /// This is the key method for already-read detection:
    /// - Returns `false` if file not tracked
    /// - Returns `false` if file was partially read (offset/limit)
    /// - Returns `false` if file was metadata-only (Glob/Grep)
    /// - Returns `true` only if full content was read AND file is unchanged
    ///
    /// # Claude Code Alignment
    ///
    /// This matches Claude Code v2.1.38's `is_already_read_unchanged` behavior:
    /// Only `FullContent` reads are considered "already read" for @mention purposes.
    /// Partial reads and metadata-only entries (from Glob/Grep) are NOT cacheable.
    pub fn is_already_read_unchanged(&self, path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        let state = self.read_guard();
        let Some(read_state) = state.read_files.peek(path) else {
            return false;
        };

        // Only full content reads are cacheable
        if !read_state.is_full() {
            return false;
        }

        drop(state); // Release lock before filesystem access in has_file_changed
        self.has_file_changed(path) == Some(false)
    }

    /// Get the read state for a file.
    ///
    /// Returns `None` if the file is not tracked.
    pub fn get_state(&self, path: impl AsRef<Path>) -> Option<FileReadState> {
        let state = self.read_guard();
        state.read_files.peek(path.as_ref()).cloned()
    }

    /// Create a snapshot of all tracked files.
    ///
    /// Used for rewind recovery - captures all file read states.
    pub fn snapshot(&self) -> Vec<(PathBuf, FileReadState)> {
        let state = self.read_guard();
        state
            .read_files
            .iter()
            .map(|(p, s)| (p.clone(), s.clone()))
            .collect()
    }

    /// Create a read-only snapshot of all tracked files.
    ///
    /// Identical to [`snapshot()`] but named to clarify intent: this is a
    /// point-in-time copy used for building derived tracker views.
    /// No LRU promotion occurs.
    pub fn read_files_snapshot(&self) -> Vec<(PathBuf, FileReadState)> {
        self.snapshot()
    }

    /// Replace all tracked files from a snapshot.
    ///
    /// Used for rewind recovery - restores file read states.
    pub fn replace_snapshot(&self, entries: Vec<(PathBuf, FileReadState)>) {
        let mut state = self.write_guard();
        state.read_files.clear();
        state.current_size_bytes = 0;

        for (path, read_state) in entries {
            let content_size = read_state.content.as_ref().map(String::len).unwrap_or(0);
            state.current_size_bytes += content_size;
            state.read_files.put(path, read_state);
        }
    }

    /// Remove tracking for multiple paths.
    ///
    /// Used for compaction cleanup - removes entries for compacted reads.
    pub fn remove_paths(&self, paths: &[PathBuf]) {
        for path in paths {
            self.remove(path);
        }
    }

    /// Clear all read file tracking (keep modified files).
    ///
    /// Used for full reset during rewind.
    pub fn clear_reads(&self) {
        let mut state = self.write_guard();
        state.read_files.clear();
        state.nested_memory_triggers.clear();
        state.current_size_bytes = 0;
    }

    /// Get the number of tracked files.
    ///
    /// Returns the count of files currently in the read cache.
    pub fn read_count(&self) -> usize {
        let state = self.read_guard();
        state.read_files.len()
    }

    // ========================================================================
    // Token Estimation (Claude Code v2.1.38 alignment)
    // ========================================================================

    /// Estimate token count for content using the canonical formula.
    ///
    /// Delegates to `cocode_protocol::estimate_text_tokens` which uses
    /// `ceil(len / 3.0)` (~3 characters per token).
    pub fn estimate_tokens(content: &str) -> usize {
        cocode_protocol::estimate_text_tokens(content) as usize
    }

    /// Estimate token count for a tracked file.
    ///
    /// Returns the estimated tokens for a file's content, or 0 if not tracked
    /// or if content is not available.
    pub fn estimate_file_tokens(&self, path: &Path) -> usize {
        let state = self.read_guard();
        state
            .read_files
            .peek(path)
            .and_then(|read_state| read_state.content.as_ref())
            .map(|c| Self::estimate_tokens(c))
            .unwrap_or(0)
    }

    /// Get total estimated tokens for all tracked files.
    ///
    /// Sums up the estimated tokens for all files with content in the tracker.
    pub fn total_estimated_tokens(&self) -> usize {
        let state = self.read_guard();
        state
            .read_files
            .iter()
            .filter_map(|(_, read_state)| read_state.content.as_ref())
            .map(|c| Self::estimate_tokens(c))
            .sum()
    }
}

#[cfg(test)]
#[path = "file_tracker.test.rs"]
mod tests;
