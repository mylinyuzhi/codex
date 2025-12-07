//! Embedding cache to avoid recomputing embeddings for unchanged content.
//!
//! Uses SQLite to persist embeddings keyed by content hash + artifact ID.
//! Artifact ID allows cache invalidation when the embedding model changes.
//!
//! Reference: Continue `core/indexing/LanceDbIndex.ts`

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use rusqlite::params;

use crate::error::Result;
use crate::error::RetrievalErr;

/// Embedding cache backed by SQLite.
///
/// Stores embeddings keyed by content hash, with artifact ID versioning
/// to invalidate cache when the embedding model changes.
pub struct EmbeddingCache {
    conn: Mutex<Connection>,
    artifact_id: String,
}

impl EmbeddingCache {
    /// Open or create an embedding cache at the given path.
    ///
    /// # Arguments
    /// * `path` - Path to the SQLite database file
    /// * `artifact_id` - Identifier for the embedding model (e.g., "text-embedding-3-small-v1")
    pub fn open(path: &Path, artifact_id: &str) -> Result<Self> {
        let conn = Connection::open(path).map_err(|e| RetrievalErr::SqliteFailed {
            operation: "open embedding cache".to_string(),
            cause: e.to_string(),
        })?;

        // Create embeddings table if it doesn't exist
        conn.execute(
            "CREATE TABLE IF NOT EXISTS embeddings (
                content_hash TEXT PRIMARY KEY,
                artifact_id TEXT NOT NULL,
                embedding BLOB NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            )",
            [],
        )
        .map_err(|e| RetrievalErr::SqliteFailed {
            operation: "create embeddings table".to_string(),
            cause: e.to_string(),
        })?;

        // Create index for efficient artifact_id lookups
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_embeddings_artifact ON embeddings(artifact_id)",
            [],
        )
        .map_err(|e| RetrievalErr::SqliteFailed {
            operation: "create artifact index".to_string(),
            cause: e.to_string(),
        })?;

        Ok(Self {
            conn: Mutex::new(conn),
            artifact_id: artifact_id.to_string(),
        })
    }

    /// Get a cached embedding for the given content hash.
    ///
    /// Returns `None` if the embedding is not cached or was created with
    /// a different artifact ID.
    pub fn get(&self, content_hash: &str) -> Option<Vec<f32>> {
        let conn = self.conn.lock().ok()?;
        conn.query_row(
            "SELECT embedding FROM embeddings WHERE content_hash = ? AND artifact_id = ?",
            params![content_hash, self.artifact_id],
            |row| {
                let bytes: Vec<u8> = row.get(0)?;
                Ok(bytes_to_f32_vec(&bytes))
            },
        )
        .ok()
    }

    /// Store an embedding in the cache.
    ///
    /// Overwrites any existing entry with the same content hash.
    pub fn put(&self, content_hash: &str, embedding: &[f32]) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| RetrievalErr::SqliteFailed {
            operation: "lock embedding cache".to_string(),
            cause: "mutex poisoned".to_string(),
        })?;

        let bytes = f32_vec_to_bytes(embedding);
        conn.execute(
            "INSERT OR REPLACE INTO embeddings (content_hash, artifact_id, embedding) VALUES (?, ?, ?)",
            params![content_hash, self.artifact_id, bytes],
        )
        .map_err(|e| RetrievalErr::SqliteFailed {
            operation: "insert embedding".to_string(),
            cause: e.to_string(),
        })?;

        Ok(())
    }

    /// Get multiple cached embeddings at once.
    ///
    /// Returns a vector of (content_hash, embedding) pairs for found entries.
    pub fn get_batch(&self, content_hashes: &[String]) -> Vec<(String, Vec<f32>)> {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        let mut results = Vec::new();
        for hash in content_hashes {
            if let Ok(embedding) = conn.query_row(
                "SELECT embedding FROM embeddings WHERE content_hash = ? AND artifact_id = ?",
                params![hash, self.artifact_id],
                |row| {
                    let bytes: Vec<u8> = row.get(0)?;
                    Ok(bytes_to_f32_vec(&bytes))
                },
            ) {
                results.push((hash.clone(), embedding));
            }
        }

        results
    }

    /// Store multiple embeddings in the cache.
    pub fn put_batch(&self, entries: &[(String, Vec<f32>)]) -> Result<()> {
        let mut conn = self.conn.lock().map_err(|_| RetrievalErr::SqliteFailed {
            operation: "lock embedding cache".to_string(),
            cause: "mutex poisoned".to_string(),
        })?;

        let tx = conn.transaction().map_err(|e| RetrievalErr::SqliteFailed {
            operation: "begin transaction".to_string(),
            cause: e.to_string(),
        })?;

        for (hash, embedding) in entries {
            let bytes = f32_vec_to_bytes(embedding);
            tx.execute(
                "INSERT OR REPLACE INTO embeddings (content_hash, artifact_id, embedding) VALUES (?, ?, ?)",
                params![hash, self.artifact_id, bytes],
            )
            .map_err(|e| RetrievalErr::SqliteFailed {
                operation: "insert embedding batch".to_string(),
                cause: e.to_string(),
            })?;
        }

        tx.commit().map_err(|e| RetrievalErr::SqliteFailed {
            operation: "commit transaction".to_string(),
            cause: e.to_string(),
        })?;

        Ok(())
    }

    /// Remove all embeddings with a different artifact ID.
    ///
    /// Useful for cleaning up stale cache entries after model upgrade.
    pub fn prune_stale(&self) -> Result<i32> {
        let conn = self.conn.lock().map_err(|_| RetrievalErr::SqliteFailed {
            operation: "lock embedding cache".to_string(),
            cause: "mutex poisoned".to_string(),
        })?;

        let count = conn
            .execute(
                "DELETE FROM embeddings WHERE artifact_id != ?",
                params![self.artifact_id],
            )
            .map_err(|e| RetrievalErr::SqliteFailed {
                operation: "prune stale embeddings".to_string(),
                cause: e.to_string(),
            })?;

        Ok(count as i32)
    }

    /// Get the total number of cached embeddings.
    pub fn count(&self) -> Result<i32> {
        let conn = self.conn.lock().map_err(|_| RetrievalErr::SqliteFailed {
            operation: "lock embedding cache".to_string(),
            cause: "mutex poisoned".to_string(),
        })?;

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM embeddings WHERE artifact_id = ?",
                params![self.artifact_id],
                |row| row.get(0),
            )
            .map_err(|e| RetrievalErr::SqliteFailed {
                operation: "count embeddings".to_string(),
                cause: e.to_string(),
            })?;

        Ok(count)
    }

    /// Get the artifact ID used by this cache.
    pub fn artifact_id(&self) -> &str {
        &self.artifact_id
    }
}

/// Convert a byte slice to a Vec<f32>.
fn bytes_to_f32_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// Convert a Vec<f32> to bytes.
fn f32_vec_to_bytes(floats: &[f32]) -> Vec<u8> {
    floats.iter().flat_map(|f| f.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_put_and_get() {
        let dir = TempDir::new().unwrap();
        let cache = EmbeddingCache::open(&dir.path().join("cache.db"), "test-model-v1").unwrap();

        let embedding = vec![0.1, 0.2, 0.3, 0.4];
        cache.put("hash123", &embedding).unwrap();

        let retrieved = cache.get("hash123").unwrap();
        assert_eq!(retrieved.len(), 4);
        assert!((retrieved[0] - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_artifact_id_isolation() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("cache.db");

        // Store with model v1
        let cache_v1 = EmbeddingCache::open(&path, "model-v1").unwrap();
        cache_v1.put("hash123", &[1.0, 2.0, 3.0]).unwrap();

        // Try to retrieve with model v2 - should not find it
        let cache_v2 = EmbeddingCache::open(&path, "model-v2").unwrap();
        assert!(cache_v2.get("hash123").is_none());

        // Original model should still work
        assert!(cache_v1.get("hash123").is_some());
    }

    #[test]
    fn test_batch_operations() {
        let dir = TempDir::new().unwrap();
        let cache = EmbeddingCache::open(&dir.path().join("cache.db"), "test-model").unwrap();

        let entries = vec![
            ("hash1".to_string(), vec![0.1, 0.2]),
            ("hash2".to_string(), vec![0.3, 0.4]),
            ("hash3".to_string(), vec![0.5, 0.6]),
        ];

        cache.put_batch(&entries).unwrap();

        let hashes: Vec<String> = vec![
            "hash1".to_string(),
            "hash2".to_string(),
            "missing".to_string(),
        ];
        let results = cache.get_batch(&hashes);

        assert_eq!(results.len(), 2); // Only hash1 and hash2 found
    }

    #[test]
    fn test_prune_stale() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("cache.db");

        // Store with old model
        let cache_old = EmbeddingCache::open(&path, "model-old").unwrap();
        cache_old.put("hash1", &[1.0]).unwrap();
        cache_old.put("hash2", &[2.0]).unwrap();

        // Store with new model
        let cache_new = EmbeddingCache::open(&path, "model-new").unwrap();
        cache_new.put("hash3", &[3.0]).unwrap();

        // Prune old entries
        let pruned = cache_new.prune_stale().unwrap();
        assert_eq!(pruned, 2);

        // Verify old entries are gone
        assert!(cache_old.get("hash1").is_none());
        // New entries remain
        assert!(cache_new.get("hash3").is_some());
    }

    #[test]
    fn test_count() {
        let dir = TempDir::new().unwrap();
        let cache = EmbeddingCache::open(&dir.path().join("cache.db"), "test-model").unwrap();

        assert_eq!(cache.count().unwrap(), 0);

        cache.put("hash1", &[1.0]).unwrap();
        cache.put("hash2", &[2.0]).unwrap();

        assert_eq!(cache.count().unwrap(), 2);
    }

    #[test]
    fn test_byte_conversion() {
        let original = vec![0.1234, 5.6789, -1.0, 0.0];
        let bytes = f32_vec_to_bytes(&original);
        let converted = bytes_to_f32_vec(&bytes);

        assert_eq!(original.len(), converted.len());
        for (a, b) in original.iter().zip(converted.iter()) {
            assert!((a - b).abs() < 0.0001);
        }
    }
}
