//! Extended cache operations for bulk lookups.
//!
//! Provides optimized batch operations using SQL WHERE IN clause
//! instead of sequential individual queries.

use std::collections::HashSet;

use super::cache::EmbeddingCache;
use super::cache::bytes_to_f32_vec;
use crate::error::Result;
use crate::error::RetrievalErr;

/// Result of a bulk cache lookup.
///
/// Separates cache hits from misses for efficient processing.
#[derive(Debug, Default)]
pub struct CacheLookupResult {
    /// Found entries: (filepath, content_hash, embedding)
    pub hits: Vec<(String, String, Vec<f32>)>,
    /// Missing entries: (filepath, content_hash)
    pub misses: Vec<(String, String)>,
}

impl CacheLookupResult {
    /// Returns true if all entries were found in cache.
    pub fn all_hits(&self) -> bool {
        self.misses.is_empty()
    }

    /// Returns true if no entries were found in cache.
    pub fn all_misses(&self) -> bool {
        self.hits.is_empty()
    }

    /// Total number of entries queried.
    pub fn total(&self) -> usize {
        self.hits.len() + self.misses.len()
    }

    /// Cache hit ratio (0.0 to 1.0).
    pub fn hit_ratio(&self) -> f32 {
        let total = self.total();
        if total == 0 {
            0.0
        } else {
            self.hits.len() as f32 / total as f32
        }
    }
}

impl EmbeddingCache {
    /// Bulk lookup using SQL WHERE IN clause.
    ///
    /// More efficient than sequential queries for large batches.
    /// Returns both hits and misses for efficient downstream processing.
    ///
    /// # Arguments
    /// * `entries` - Slice of (filepath, content_hash) pairs to look up
    ///
    /// # Returns
    /// `CacheLookupResult` with hits (found embeddings) and misses (not found)
    pub fn get_batch_bulk(&self, entries: &[(String, String)]) -> Result<CacheLookupResult> {
        if entries.is_empty() {
            return Ok(CacheLookupResult::default());
        }

        // Clone entries for the closure
        let entries_clone: Vec<(String, String)> = entries.to_vec();

        self.with_conn(|conn, artifact_id| {
            // Build parameterized query with placeholders
            // SQLite doesn't support tuple IN directly, so we use OR conditions
            let mut conditions = Vec::with_capacity(entries_clone.len());
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

            for (filepath, hash) in &entries_clone {
                conditions.push("(filepath = ? AND content_hash = ?)");
                params_vec.push(Box::new(filepath.clone()));
                params_vec.push(Box::new(hash.clone()));
            }

            let query = format!(
                "SELECT filepath, content_hash, embedding FROM embeddings WHERE artifact_id = ? AND ({})",
                conditions.join(" OR ")
            );

            // Add artifact_id as first parameter
            let mut all_params: Vec<Box<dyn rusqlite::ToSql>> =
                vec![Box::new(artifact_id.to_string())];
            all_params.extend(params_vec);

            // Convert to references for rusqlite
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                all_params.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn
                .prepare(&query)
                .map_err(|e| RetrievalErr::SqliteFailed {
                    operation: "prepare bulk lookup".to_string(),
                    cause: e.to_string(),
                })?;

            let rows = stmt
                .query_map(params_refs.as_slice(), |row| {
                    let filepath: String = row.get(0)?;
                    let content_hash: String = row.get(1)?;
                    let bytes: Vec<u8> = row.get(2)?;
                    Ok((filepath, content_hash, bytes_to_f32_vec(&bytes)))
                })
                .map_err(|e| RetrievalErr::SqliteFailed {
                    operation: "execute bulk lookup".to_string(),
                    cause: e.to_string(),
                })?;

            // Collect hits
            let mut hits = Vec::new();
            let mut found_keys: HashSet<(String, String)> = HashSet::new();

            for row_result in rows {
                if let Ok((filepath, hash, embedding)) = row_result {
                    found_keys.insert((filepath.clone(), hash.clone()));
                    hits.push((filepath, hash, embedding));
                }
            }

            // Determine misses
            let misses: Vec<(String, String)> = entries_clone
                .iter()
                .filter(|(f, h)| !found_keys.contains(&(f.clone(), h.clone())))
                .cloned()
                .collect();

            Ok(CacheLookupResult { hits, misses })
        })
    }

    /// Lookup with deduplication by content hash.
    ///
    /// When multiple files have identical content, only one embedding needs to
    /// be computed. This method returns unique content hashes that need embedding.
    ///
    /// # Returns
    /// Tuple of (cached embeddings, unique content hashes needing embedding)
    pub fn get_batch_deduplicated(
        &self,
        entries: &[(String, String)],
    ) -> Result<(Vec<(String, String, Vec<f32>)>, Vec<String>)> {
        let result = self.get_batch_bulk(entries)?;

        // Collect unique content hashes from misses
        let mut unique_hashes: HashSet<String> = HashSet::new();
        for (_, hash) in &result.misses {
            unique_hashes.insert(hash.clone());
        }

        Ok((result.hits, unique_hashes.into_iter().collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_cache() -> (TempDir, EmbeddingCache) {
        let dir = TempDir::new().unwrap();
        let cache = EmbeddingCache::open(&dir.path().join("cache.db"), "test-model").unwrap();
        (dir, cache)
    }

    #[test]
    fn test_bulk_lookup_empty() {
        let (_dir, cache) = create_test_cache();
        let result = cache.get_batch_bulk(&[]).unwrap();
        assert!(result.all_hits()); // Empty is considered all hits
        assert_eq!(result.total(), 0);
    }

    #[test]
    fn test_bulk_lookup_all_hits() {
        let (_dir, cache) = create_test_cache();

        // Insert some entries
        cache.put("file1.rs", "hash1", &[0.1, 0.2]).unwrap();
        cache.put("file2.rs", "hash2", &[0.3, 0.4]).unwrap();

        let entries = vec![
            ("file1.rs".to_string(), "hash1".to_string()),
            ("file2.rs".to_string(), "hash2".to_string()),
        ];

        let result = cache.get_batch_bulk(&entries).unwrap();
        assert!(result.all_hits());
        assert_eq!(result.hits.len(), 2);
        assert_eq!(result.misses.len(), 0);
        assert!((result.hit_ratio() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_bulk_lookup_all_misses() {
        let (_dir, cache) = create_test_cache();

        let entries = vec![
            ("missing1.rs".to_string(), "hash1".to_string()),
            ("missing2.rs".to_string(), "hash2".to_string()),
        ];

        let result = cache.get_batch_bulk(&entries).unwrap();
        assert!(result.all_misses());
        assert_eq!(result.hits.len(), 0);
        assert_eq!(result.misses.len(), 2);
        assert!(result.hit_ratio() < 0.001);
    }

    #[test]
    fn test_bulk_lookup_mixed() {
        let (_dir, cache) = create_test_cache();

        // Insert only one entry
        cache.put("found.rs", "hash1", &[0.1, 0.2]).unwrap();

        let entries = vec![
            ("found.rs".to_string(), "hash1".to_string()),
            ("missing.rs".to_string(), "hash2".to_string()),
        ];

        let result = cache.get_batch_bulk(&entries).unwrap();
        assert!(!result.all_hits());
        assert!(!result.all_misses());
        assert_eq!(result.hits.len(), 1);
        assert_eq!(result.misses.len(), 1);
        assert!((result.hit_ratio() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_deduplicated_lookup() {
        let (_dir, cache) = create_test_cache();

        // Two files with same content hash
        let entries = vec![
            ("file_a.rs".to_string(), "same_hash".to_string()),
            ("file_b.rs".to_string(), "same_hash".to_string()),
            ("file_c.rs".to_string(), "different_hash".to_string()),
        ];

        let (hits, unique_hashes) = cache.get_batch_deduplicated(&entries).unwrap();
        assert!(hits.is_empty()); // Nothing cached
        assert_eq!(unique_hashes.len(), 2); // Only 2 unique hashes
        assert!(unique_hashes.contains(&"same_hash".to_string()));
        assert!(unique_hashes.contains(&"different_hash".to_string()));
    }

    #[test]
    fn test_hit_ratio() {
        let result = CacheLookupResult {
            hits: vec![
                ("a".to_string(), "h1".to_string(), vec![0.1]),
                ("b".to_string(), "h2".to_string(), vec![0.2]),
            ],
            misses: vec![("c".to_string(), "h3".to_string())],
        };
        // 2 hits, 1 miss = 2/3 ≈ 0.667
        assert!((result.hit_ratio() - 0.667).abs() < 0.01);
    }
}
