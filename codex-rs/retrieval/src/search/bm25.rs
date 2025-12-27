//! BM25 full-text search.
//!
//! Uses LanceDB's built-in FTS capabilities.

use std::sync::Arc;

use crate::error::Result;
use crate::storage::LanceDbStore;
use crate::types::SearchQuery;
use crate::types::SearchResult;

/// BM25 searcher using LanceDB FTS.
pub struct Bm25Searcher {
    store: Arc<LanceDbStore>,
}

impl Bm25Searcher {
    /// Create a new BM25 searcher.
    pub fn new(store: Arc<LanceDbStore>) -> Self {
        Self { store }
    }

    /// Search for code chunks matching the query.
    pub async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let chunks = self.store.search_fts(&query.text, query.limit).await?;

        Ok(chunks
            .into_iter()
            .enumerate()
            .map(|(i, chunk)| SearchResult {
                chunk,
                score: 1.0 / (1.0 + i as f32), // Simple ranking for now
                score_type: crate::types::ScoreType::Bm25,
                is_stale: None,
            })
            .collect())
    }
}
