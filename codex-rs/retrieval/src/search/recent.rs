//! Recently Edited Files Retrieval.
//!
//! LRU cache for tracking recently accessed files and their chunks.
//! Provides temporal relevance signal for search results.
//!
//! Reference: Continue's `BaseRetrievalPipeline.ts:141`

use crate::types::CodeChunk;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

/// Entry in the recent files cache.
#[derive(Debug, Clone)]
pub struct RecentFileEntry {
    /// File path (relative to workspace)
    pub path: PathBuf,
    /// Last accessed timestamp
    pub last_accessed: Instant,
    /// Cached code chunks for this file
    pub chunks: Vec<CodeChunk>,
}

/// LRU cache for recently edited/accessed files.
///
/// Tracks files that have been recently accessed and stores their chunks
/// for quick retrieval. This provides a temporal relevance signal that
/// complements semantic and lexical search.
pub struct RecentFilesCache {
    cache: LruCache<PathBuf, RecentFileEntry>,
}

impl RecentFilesCache {
    /// Create a new recent files cache with the specified capacity.
    ///
    /// # Arguments
    /// * `max_entries` - Maximum number of files to track (LRU eviction)
    pub fn new(max_entries: usize) -> Self {
        let capacity = NonZeroUsize::new(max_entries).unwrap_or(NonZeroUsize::MIN);
        Self {
            cache: LruCache::new(capacity),
        }
    }

    /// Notify the cache that a file has been accessed or edited.
    ///
    /// This should be called when:
    /// - A file is opened in the editor
    /// - A file is modified
    /// - A file is explicitly marked as relevant
    ///
    /// # Arguments
    /// * `path` - File path (relative to workspace)
    /// * `chunks` - Pre-computed chunks for this file
    pub fn notify_file_accessed(&mut self, path: &Path, chunks: Vec<CodeChunk>) {
        let entry = RecentFileEntry {
            path: path.to_path_buf(),
            last_accessed: Instant::now(),
            chunks,
        };
        self.cache.put(path.to_path_buf(), entry);
    }

    /// Update chunks for an existing file without changing its position.
    ///
    /// Returns false if the file is not in the cache.
    pub fn update_chunks(&mut self, path: &Path, chunks: Vec<CodeChunk>) -> bool {
        if let Some(entry) = self.cache.get_mut(&path.to_path_buf()) {
            entry.chunks = chunks;
            true
        } else {
            false
        }
    }

    /// Mark a file as accessed (touch) without updating chunks.
    ///
    /// Moves the file to the front of the LRU if it exists.
    /// Returns false if the file is not in the cache.
    pub fn touch(&mut self, path: &Path) -> bool {
        if let Some(entry) = self.cache.get_mut(&path.to_path_buf()) {
            entry.last_accessed = Instant::now();
            true
        } else {
            false
        }
    }

    /// Remove a file from the cache.
    ///
    /// Call this when a file is deleted or closed.
    pub fn remove(&mut self, path: &Path) -> Option<RecentFileEntry> {
        self.cache.pop(&path.to_path_buf())
    }

    /// Retrieve chunks from recently accessed files.
    ///
    /// Returns chunks from the most recently accessed files first,
    /// up to the specified limit.
    ///
    /// # Arguments
    /// * `n` - Maximum number of chunks to return
    pub fn retrieve(&self, n: usize) -> Vec<CodeChunk> {
        self.cache
            .iter()
            .flat_map(|(_, entry)| entry.chunks.iter().cloned())
            .take(n)
            .collect()
    }

    /// Retrieve chunks with file age information.
    ///
    /// Returns (chunk, age_in_seconds) pairs for scoring purposes.
    pub fn retrieve_with_age(&self, n: usize) -> Vec<(CodeChunk, u64)> {
        let now = Instant::now();
        self.cache
            .iter()
            .flat_map(|(_, entry)| {
                let age_secs = now.duration_since(entry.last_accessed).as_secs();
                entry
                    .chunks
                    .iter()
                    .cloned()
                    .map(move |chunk| (chunk, age_secs))
            })
            .take(n)
            .collect()
    }

    /// Get all files currently in the cache.
    pub fn files(&self) -> Vec<PathBuf> {
        self.cache.iter().map(|(path, _)| path.clone()).collect()
    }

    /// Get the number of files in the cache.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Check if a file is in the cache.
    pub fn contains(&self, path: &Path) -> bool {
        self.cache.contains(&path.to_path_buf())
    }

    /// Clear all entries from the cache.
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Get entry for a specific file if it exists.
    pub fn get(&self, path: &Path) -> Option<&RecentFileEntry> {
        // peek() doesn't change LRU order
        self.cache.peek(&path.to_path_buf())
    }
}

impl Default for RecentFilesCache {
    fn default() -> Self {
        // Default capacity: 50 files
        Self::new(50)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    fn make_chunk(id: &str, filepath: &str) -> CodeChunk {
        CodeChunk {
            id: id.to_string(),
            source_id: "test".to_string(),
            filepath: filepath.to_string(),
            language: "rust".to_string(),
            content: format!("content of {id}"),
            start_line: 1,
            end_line: 10,
            embedding: None,
            modified_time: None,
            workspace: "test".to_string(),
            content_hash: String::new(),
            indexed_at: 0,
            parent_symbol: None,
            is_overview: false,
        }
    }

    #[test]
    fn test_new_cache() {
        let cache = RecentFilesCache::new(10);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_notify_file_accessed() {
        let mut cache = RecentFilesCache::new(10);
        let path = Path::new("src/main.rs");
        let chunks = vec![make_chunk("chunk1", "src/main.rs")];

        cache.notify_file_accessed(path, chunks.clone());

        assert!(!cache.is_empty());
        assert_eq!(cache.len(), 1);
        assert!(cache.contains(path));
    }

    #[test]
    fn test_retrieve_chunks() {
        let mut cache = RecentFilesCache::new(10);

        // Add multiple files
        let path1 = Path::new("src/main.rs");
        let path2 = Path::new("src/lib.rs");

        cache.notify_file_accessed(
            path1,
            vec![
                make_chunk("main:0", "src/main.rs"),
                make_chunk("main:1", "src/main.rs"),
            ],
        );
        cache.notify_file_accessed(path2, vec![make_chunk("lib:0", "src/lib.rs")]);

        // Retrieve all
        let chunks = cache.retrieve(100);
        assert_eq!(chunks.len(), 3);

        // Retrieve limited
        let chunks = cache.retrieve(2);
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn test_lru_eviction() {
        let mut cache = RecentFilesCache::new(2);

        // Add 3 files to a cache with capacity 2
        cache.notify_file_accessed(Path::new("a.rs"), vec![make_chunk("a:0", "a.rs")]);
        cache.notify_file_accessed(Path::new("b.rs"), vec![make_chunk("b:0", "b.rs")]);
        cache.notify_file_accessed(Path::new("c.rs"), vec![make_chunk("c:0", "c.rs")]);

        // Oldest (a.rs) should be evicted
        assert!(!cache.contains(Path::new("a.rs")));
        assert!(cache.contains(Path::new("b.rs")));
        assert!(cache.contains(Path::new("c.rs")));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_touch_updates_lru_order() {
        let mut cache = RecentFilesCache::new(2);

        cache.notify_file_accessed(Path::new("a.rs"), vec![make_chunk("a:0", "a.rs")]);
        cache.notify_file_accessed(Path::new("b.rs"), vec![make_chunk("b:0", "b.rs")]);

        // Touch a.rs to make it most recent
        assert!(cache.touch(Path::new("a.rs")));

        // Add c.rs - should evict b.rs (now oldest)
        cache.notify_file_accessed(Path::new("c.rs"), vec![make_chunk("c:0", "c.rs")]);

        assert!(cache.contains(Path::new("a.rs")));
        assert!(!cache.contains(Path::new("b.rs")));
        assert!(cache.contains(Path::new("c.rs")));
    }

    #[test]
    fn test_update_chunks() {
        let mut cache = RecentFilesCache::new(10);
        let path = Path::new("src/main.rs");

        cache.notify_file_accessed(path, vec![make_chunk("old", "src/main.rs")]);

        // Update chunks
        let updated = cache.update_chunks(path, vec![make_chunk("new", "src/main.rs")]);
        assert!(updated);

        let chunks = cache.retrieve(10);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].id, "new");
    }

    #[test]
    fn test_update_chunks_nonexistent() {
        let mut cache = RecentFilesCache::new(10);
        let updated = cache.update_chunks(Path::new("nonexistent.rs"), vec![]);
        assert!(!updated);
    }

    #[test]
    fn test_remove() {
        let mut cache = RecentFilesCache::new(10);
        let path = Path::new("src/main.rs");

        cache.notify_file_accessed(path, vec![make_chunk("chunk", "src/main.rs")]);
        assert!(cache.contains(path));

        let removed = cache.remove(path);
        assert!(removed.is_some());
        assert!(!cache.contains(path));
    }

    #[test]
    fn test_retrieve_with_age() {
        let mut cache = RecentFilesCache::new(10);
        let path = Path::new("src/main.rs");

        cache.notify_file_accessed(path, vec![make_chunk("chunk", "src/main.rs")]);

        // Small sleep to ensure age > 0
        sleep(Duration::from_millis(10));

        let results = cache.retrieve_with_age(10);
        assert_eq!(results.len(), 1);
        // Age should be 0 seconds (sub-second sleep)
        assert_eq!(results[0].1, 0);
    }

    #[test]
    fn test_files_list() {
        let mut cache = RecentFilesCache::new(10);

        cache.notify_file_accessed(Path::new("a.rs"), vec![make_chunk("a", "a.rs")]);
        cache.notify_file_accessed(Path::new("b.rs"), vec![make_chunk("b", "b.rs")]);

        let files = cache.files();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_clear() {
        let mut cache = RecentFilesCache::new(10);

        cache.notify_file_accessed(Path::new("a.rs"), vec![make_chunk("a", "a.rs")]);
        cache.notify_file_accessed(Path::new("b.rs"), vec![make_chunk("b", "b.rs")]);

        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_default() {
        let cache = RecentFilesCache::default();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_get_entry() {
        let mut cache = RecentFilesCache::new(10);
        let path = Path::new("src/main.rs");

        cache.notify_file_accessed(path, vec![make_chunk("chunk", "src/main.rs")]);

        let entry = cache.get(path);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().path, path);
        assert_eq!(entry.unwrap().chunks.len(), 1);
    }

    #[test]
    fn test_mru_order_in_retrieve() {
        let mut cache = RecentFilesCache::new(10);

        // Add files in order: a, b, c
        cache.notify_file_accessed(Path::new("a.rs"), vec![make_chunk("a:0", "a.rs")]);
        cache.notify_file_accessed(Path::new("b.rs"), vec![make_chunk("b:0", "b.rs")]);
        cache.notify_file_accessed(Path::new("c.rs"), vec![make_chunk("c:0", "c.rs")]);

        // Most recently added (c) should come first in iteration
        let chunks = cache.retrieve(10);
        // LRU iteration order is most-recently-used first
        assert_eq!(chunks[0].filepath, "c.rs");
    }
}
