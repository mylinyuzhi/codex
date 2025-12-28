//! 3-level cache for repo map.
//!
//! Level 1: SQLite - persistent tag cache (filepath, mtime) -> Vec<CodeTag>
//! Level 2: In-memory LRU - tree cache for rendered output
//! Level 3: In-memory TTL - full map result cache

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use lru::LruCache;
use std::num::NonZeroUsize;

use crate::error::Result;
use crate::storage::SqliteStore;
use crate::tags::extractor::CodeTag;
use crate::tags::extractor::TagKind;

/// 3-level cache for repo map operations.
pub struct RepoMapCache {
    /// SQLite store for persistent tag caching
    db: Arc<SqliteStore>,
    /// In-memory LRU for tree rendering cache
    tree_cache: LruCache<TreeCacheKey, String>,
    /// In-memory TTL cache for full map results
    map_cache: HashMap<MapCacheKey, (MapCacheEntry, Instant)>,
    /// TTL for map cache entries in seconds
    cache_ttl_secs: i64,
}

/// Key for tree cache (LRU level 2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TreeCacheKey {
    filepath: String,
    line_count: i32,
}

/// Key for map cache (TTL level 3).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MapCacheKey {
    request_hash: String,
}

/// Entry for map cache.
#[derive(Debug, Clone)]
struct MapCacheEntry {
    content: String,
    tokens: i32,
    files_included: i32,
}

impl RepoMapCache {
    /// Create a new 3-level cache.
    pub fn new(db: Arc<SqliteStore>, cache_ttl_secs: i64) -> Self {
        Self {
            db,
            tree_cache: LruCache::new(NonZeroUsize::new(1000).unwrap()),
            map_cache: HashMap::new(),
            cache_ttl_secs,
        }
    }

    // ========== Level 1: SQLite Tag Cache ==========

    /// Get cached tags for a file.
    pub async fn get_tags(&self, filepath: &str) -> Result<Option<Vec<CodeTag>>> {
        let fp = filepath.to_string();

        self.db
            .query(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT name, kind, line FROM repomap_tags WHERE filepath = ? ORDER BY line",
                )?;

                let rows = stmt.query_map([&fp], |row| {
                    let name: String = row.get(0)?;
                    let kind_str: String = row.get(1)?;
                    let line: i32 = row.get(2)?;

                    Ok((name, kind_str, line))
                })?;

                let mut tags = Vec::new();
                for row in rows {
                    let (name, kind_str, line) = row?;
                    let is_definition = kind_str == "def";

                    tags.push(CodeTag {
                        name,
                        kind: TagKind::Function, // Simplified - could store full kind
                        start_line: line,
                        end_line: line,
                        start_byte: 0, // Not stored in cache
                        end_byte: 0,
                        signature: None,
                        docs: None,
                        is_definition,
                    });
                }

                if tags.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(tags))
                }
            })
            .await
    }

    /// Store tags for a file.
    pub async fn put_tags(&self, filepath: &str, tags: &[CodeTag]) -> Result<()> {
        let fp = filepath.to_string();
        let tags_clone: Vec<(String, String, i32)> = tags
            .iter()
            .map(|t| {
                let kind = if t.is_definition { "def" } else { "ref" };
                (t.name.clone(), kind.to_string(), t.start_line)
            })
            .collect();

        self.db
            .transaction(move |conn| {
                // Delete old entries
                conn.execute("DELETE FROM repomap_tags WHERE filepath = ?", [&fp])?;

                // Insert new entries
                let mut stmt = conn.prepare(
                    "INSERT INTO repomap_tags (workspace, filepath, mtime, name, kind, line)
                     VALUES ('default', ?, strftime('%s', 'now'), ?, ?, ?)",
                )?;

                for (name, kind, line) in &tags_clone {
                    stmt.execute(rusqlite::params![fp, name, kind, line])?;
                }

                Ok(())
            })
            .await
    }

    /// Invalidate cached tags for a file.
    pub async fn invalidate_tags(&self, filepath: &str) -> Result<()> {
        let fp = filepath.to_string();

        self.db
            .query(move |conn| {
                conn.execute("DELETE FROM repomap_tags WHERE filepath = ?", [&fp])?;
                Ok(())
            })
            .await
    }

    // ========== Level 2: In-Memory LRU Tree Cache ==========

    /// Get cached tree rendering for a file.
    #[allow(dead_code)]
    pub fn get_tree(&mut self, filepath: &str, line_count: i32) -> Option<String> {
        let key = TreeCacheKey {
            filepath: filepath.to_string(),
            line_count,
        };
        self.tree_cache.get(&key).cloned()
    }

    /// Store tree rendering for a file.
    #[allow(dead_code)]
    pub fn put_tree(&mut self, filepath: &str, line_count: i32, content: String) {
        let key = TreeCacheKey {
            filepath: filepath.to_string(),
            line_count,
        };
        self.tree_cache.put(key, content);
    }

    /// Invalidate tree cache for a file.
    #[allow(dead_code)]
    pub fn invalidate_tree(&mut self, filepath: &str) {
        // Remove all entries for this filepath
        let keys_to_remove: Vec<TreeCacheKey> = self
            .tree_cache
            .iter()
            .filter(|(k, _)| k.filepath == filepath)
            .map(|(k, _)| k.clone())
            .collect();

        for key in keys_to_remove {
            self.tree_cache.pop(&key);
        }
    }

    // ========== Level 3: In-Memory TTL Map Cache ==========

    /// Get cached map result by request hash.
    #[allow(dead_code)]
    pub fn get_map(&mut self, request_hash: &str) -> Option<(String, i32, i32)> {
        self.cleanup_expired();

        let key = MapCacheKey {
            request_hash: request_hash.to_string(),
        };

        self.map_cache
            .get(&key)
            .map(|(entry, _)| (entry.content.clone(), entry.tokens, entry.files_included))
    }

    /// Store map result.
    #[allow(dead_code)]
    pub fn put_map(
        &mut self,
        request_hash: &str,
        content: String,
        tokens: i32,
        files_included: i32,
    ) {
        let key = MapCacheKey {
            request_hash: request_hash.to_string(),
        };

        let entry = MapCacheEntry {
            content,
            tokens,
            files_included,
        };

        self.map_cache.insert(key, (entry, Instant::now()));
    }

    /// Invalidate all map cache entries.
    #[allow(dead_code)]
    pub fn invalidate_all_maps(&mut self) {
        self.map_cache.clear();
    }

    /// Clean up expired TTL entries.
    fn cleanup_expired(&mut self) {
        let ttl = std::time::Duration::from_secs(self.cache_ttl_secs as u64);
        let now = Instant::now();

        self.map_cache
            .retain(|_, (_, created_at)| now.duration_since(*created_at) < ttl);
    }

    // ========== Utility ==========

    /// Get cache statistics.
    #[allow(dead_code)]
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            tree_cache_size: self.tree_cache.len(),
            map_cache_size: self.map_cache.len(),
        }
    }
}

/// Cache statistics.
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of entries in tree cache
    pub tree_cache_size: usize,
    /// Number of entries in map cache
    pub map_cache_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup() -> (TempDir, RepoMapCache) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let store = Arc::new(SqliteStore::open(&db_path).unwrap());
        let cache = RepoMapCache::new(store, 3600);
        (dir, cache)
    }

    fn make_tag(name: &str, line: i32, is_def: bool) -> CodeTag {
        CodeTag {
            name: name.to_string(),
            kind: TagKind::Function,
            start_line: line,
            end_line: line + 10,
            start_byte: line * 100,
            end_byte: (line + 10) * 100,
            signature: Some(format!("fn {}()", name)),
            docs: None,
            is_definition: is_def,
        }
    }

    #[tokio::test]
    async fn test_tag_cache() {
        let (_dir, cache) = setup().await;

        // Initially empty
        let result = cache.get_tags("test.rs").await.unwrap();
        assert!(result.is_none());

        // Store tags
        let tags = vec![make_tag("foo", 10, true), make_tag("bar", 20, false)];
        cache.put_tags("test.rs", &tags).await.unwrap();

        // Retrieve tags
        let result = cache.get_tags("test.rs").await.unwrap();
        assert!(result.is_some());
        let cached_tags = result.unwrap();
        assert_eq!(cached_tags.len(), 2);
        assert_eq!(cached_tags[0].name, "foo");
        assert!(cached_tags[0].is_definition);
        assert_eq!(cached_tags[1].name, "bar");
        assert!(!cached_tags[1].is_definition);

        // Invalidate
        cache.invalidate_tags("test.rs").await.unwrap();
        let result = cache.get_tags("test.rs").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_tree_cache() {
        let (_dir, mut cache) = setup().await;

        // Initially empty
        let result = cache.get_tree("test.rs", 100);
        assert!(result.is_none());

        // Store tree
        cache.put_tree("test.rs", 100, "rendered content".to_string());

        // Retrieve tree
        let result = cache.get_tree("test.rs", 100);
        assert_eq!(result, Some("rendered content".to_string()));

        // Different line count = miss
        let result = cache.get_tree("test.rs", 200);
        assert!(result.is_none());

        // Invalidate
        cache.invalidate_tree("test.rs");
        let result = cache.get_tree("test.rs", 100);
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_map_cache() {
        let (_dir, mut cache) = setup().await;

        // Initially empty
        let result = cache.get_map("hash123");
        assert!(result.is_none());

        // Store map
        cache.put_map("hash123", "map content".to_string(), 100, 5);

        // Retrieve map
        let result = cache.get_map("hash123");
        assert!(result.is_some());
        let (content, tokens, files) = result.unwrap();
        assert_eq!(content, "map content");
        assert_eq!(tokens, 100);
        assert_eq!(files, 5);

        // Invalidate all
        cache.invalidate_all_maps();
        let result = cache.get_map("hash123");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let (_dir, mut cache) = setup().await;

        let stats = cache.stats();
        assert_eq!(stats.tree_cache_size, 0);
        assert_eq!(stats.map_cache_size, 0);

        cache.put_tree("a.rs", 10, "content".to_string());
        cache.put_map("hash1", "map".to_string(), 50, 2);

        let stats = cache.stats();
        assert_eq!(stats.tree_cache_size, 1);
        assert_eq!(stats.map_cache_size, 1);
    }
}
