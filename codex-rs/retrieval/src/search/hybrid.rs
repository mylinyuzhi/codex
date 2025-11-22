//! Hybrid search combining BM25 and vector search.
//!
//! Uses RRF (Reciprocal Rank Fusion) to combine results from
//! multiple search methods.
//!
//! Optionally applies rule-based reranking for improved relevance.

use std::sync::Arc;

use crate::config::RerankerConfig;
use crate::error::Result;
use crate::reranker::Reranker;
use crate::reranker::RuleBasedReranker;
use crate::reranker::RuleBasedRerankerConfig;
use crate::search::dedup::deduplicate_results;
use crate::search::dedup::limit_chunks_per_file;
use crate::search::fusion::RrfConfig;
use crate::search::fusion::fuse_all_results;
use crate::search::fusion::has_symbol_syntax;
use crate::search::fusion::is_identifier_query;
use crate::search::hybrid_ext::SnippetSearcher;
use crate::storage::SqliteStore;
use crate::storage::lancedb::LanceDbStore;
use crate::traits::EmbeddingProvider;
use crate::types::ScoreType;
use crate::types::SearchResult;

/// Hybrid searcher combining BM25 and vector search.
pub struct HybridSearcher {
    store: Arc<LanceDbStore>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    config: RrfConfig,
    /// Maximum chunks per file (0 = unlimited)
    max_chunks_per_file: usize,
    /// Optional reranker for post-retrieval score adjustment
    reranker: Option<RuleBasedReranker>,
    /// Optional snippet searcher for symbol-based search
    snippet_searcher: Option<SnippetSearcher>,
}

impl HybridSearcher {
    /// Create a new hybrid searcher with BM25 only.
    pub fn new(store: Arc<LanceDbStore>) -> Self {
        Self {
            store,
            embedding_provider: None,
            config: RrfConfig::default(),
            max_chunks_per_file: 2, // Default from Tabby
            reranker: None,
            snippet_searcher: None,
        }
    }

    /// Create a hybrid searcher with vector search enabled.
    pub fn with_embeddings(store: Arc<LanceDbStore>, provider: Arc<dyn EmbeddingProvider>) -> Self {
        Self {
            store,
            embedding_provider: Some(provider),
            config: RrfConfig::default(),
            max_chunks_per_file: 2, // Default from Tabby
            reranker: None,
            snippet_searcher: None,
        }
    }

    /// Enable snippet search for symbol-based queries.
    ///
    /// When enabled, queries containing `type:` or `name:` syntax will
    /// use the snippet index for symbol matching.
    pub fn with_snippet_search(mut self, sqlite_store: Arc<SqliteStore>, workspace: &str) -> Self {
        self.snippet_searcher = Some(SnippetSearcher::new(sqlite_store, workspace));
        self
    }

    /// Disable snippet search.
    pub fn without_snippet_search(mut self) -> Self {
        self.snippet_searcher = None;
        self
    }

    /// Check if snippet search is available.
    pub fn has_snippet_search(&self) -> bool {
        self.snippet_searcher.is_some()
    }

    /// Set custom RRF configuration.
    pub fn with_config(mut self, config: RrfConfig) -> Self {
        self.config = config;
        self
    }

    /// Set maximum chunks per file (0 = unlimited).
    pub fn with_max_chunks_per_file(mut self, max: usize) -> Self {
        self.max_chunks_per_file = max;
        self
    }

    /// Enable reranking with the given configuration.
    ///
    /// If config.enabled is false, reranking will be disabled.
    pub fn with_reranker_config(mut self, config: &RerankerConfig) -> Self {
        if config.enabled {
            let reranker_config = RuleBasedRerankerConfig {
                exact_match_boost: config.exact_match_boost,
                path_relevance_boost: config.path_relevance_boost,
                recency_boost: config.recency_boost,
                recency_days_threshold: config.recency_days_threshold,
            };
            self.reranker = Some(RuleBasedReranker::with_config(reranker_config));
        } else {
            self.reranker = None;
        }
        self
    }

    /// Enable reranking with default configuration.
    pub fn with_reranker(mut self) -> Self {
        self.reranker = Some(RuleBasedReranker::new());
        self
    }

    /// Disable reranking.
    pub fn without_reranker(mut self) -> Self {
        self.reranker = None;
        self
    }

    /// Check if reranking is enabled.
    pub fn has_reranker(&self) -> bool {
        self.reranker.is_some()
    }

    /// Search using hybrid (BM25 + vector + snippet) search.
    ///
    /// If no embedding provider is configured, falls back to BM25-only search.
    /// If snippet search is enabled and query contains symbol syntax, uses snippet search.
    /// If reranking is enabled, applies rule-based reranking after fusion.
    pub async fn search(&self, query: &str, limit: i32) -> Result<Vec<SearchResult>> {
        // Detect query type once to avoid repeated parsing
        let is_symbol = has_symbol_syntax(query);
        let is_identifier = !is_symbol && is_identifier_query(query);

        // Adjust config based on query type
        let config = if is_symbol {
            self.config.clone().for_symbol_query()
        } else if is_identifier {
            self.config.clone().for_identifier_query()
        } else {
            self.config.clone()
        };

        // Get BM25 results
        let bm25_results = self.search_bm25(query, limit * 2).await?;

        // Get vector results if embedding provider is available
        let vector_results = if let Some(ref provider) = self.embedding_provider {
            match self
                .search_vector(query, provider.as_ref(), limit * 2)
                .await
            {
                Ok(results) => results,
                Err(e) => {
                    // Log warning and fall back to BM25 only
                    tracing::warn!("Vector search failed, falling back to BM25: {e}");
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        // Get snippet results if snippet searcher is available and useful
        let snippet_results = if let Some(ref searcher) = self.snippet_searcher {
            // Search snippets if config has snippet weight or query has symbol syntax
            if config.snippet_weight > 0.0 || is_symbol {
                match searcher.search(query, limit * 2).await {
                    Ok(results) => results,
                    Err(e) => {
                        tracing::warn!("Snippet search failed: {e}");
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // If only BM25 results are available (no vector, no snippet), return directly
        // This preserves ScoreType::Bm25 for fallback scenarios
        if vector_results.is_empty() && snippet_results.is_empty() {
            let mut results = bm25_results;
            results.truncate(limit as usize);
            let deduped = deduplicate_results(results);
            let limited = self.apply_per_file_limit(deduped);
            return Ok(self.apply_reranking(query, limited));
        }

        // Use fuse_all_results for multi-source fusion
        let fused = fuse_all_results(
            &bm25_results,
            &vector_results,
            &snippet_results,
            &[], // recent_results - TODO: integrate RecentFilesCache
            &config,
            limit,
        );

        // Deduplicate overlapping chunks
        let deduped = deduplicate_results(fused);
        // Apply per-file limit for diversity
        let limited = self.apply_per_file_limit(deduped);
        // Apply reranking if enabled
        Ok(self.apply_reranking(query, limited))
    }

    /// Apply reranking if enabled.
    fn apply_reranking(&self, query: &str, mut results: Vec<SearchResult>) -> Vec<SearchResult> {
        if let Some(ref reranker) = self.reranker {
            reranker.rerank(query, &mut results);
        }
        results
    }

    /// Apply per-file chunk limit for result diversity.
    fn apply_per_file_limit(&self, results: Vec<SearchResult>) -> Vec<SearchResult> {
        if self.max_chunks_per_file == 0 {
            results
        } else {
            limit_chunks_per_file(results, self.max_chunks_per_file)
        }
    }

    /// Search using BM25 full-text search only.
    pub async fn search_bm25(&self, query: &str, limit: i32) -> Result<Vec<SearchResult>> {
        let chunks = self.store.search_fts(query, limit).await?;

        // Convert to SearchResult with rank-based scores
        Ok(chunks
            .into_iter()
            .enumerate()
            .map(|(i, chunk)| SearchResult {
                chunk,
                score: 1.0 / (i as f32 + 1.0), // Simple rank-based score
                score_type: ScoreType::Bm25,
            })
            .collect())
    }

    /// Search using vector similarity only.
    async fn search_vector(
        &self,
        query: &str,
        provider: &dyn EmbeddingProvider,
        limit: i32,
    ) -> Result<Vec<SearchResult>> {
        // Embed the query
        let embedding = provider.embed(query).await?;

        // Search for similar vectors
        let chunks = self.store.search_vector(&embedding, limit).await?;

        // Convert to SearchResult with rank-based scores
        Ok(chunks
            .into_iter()
            .enumerate()
            .map(|(i, chunk)| SearchResult {
                chunk,
                score: 1.0 / (i as f32 + 1.0), // Simple rank-based score
                score_type: ScoreType::Vector,
            })
            .collect())
    }

    /// Check if vector search is available.
    pub fn has_vector_search(&self) -> bool {
        self.embedding_provider.is_some()
    }

    /// Search using vector similarity only (public API).
    ///
    /// Returns empty results if no embedding provider is configured.
    pub async fn search_vector_only(&self, query: &str, limit: i32) -> Result<Vec<SearchResult>> {
        let Some(ref provider) = self.embedding_provider else {
            return Ok(Vec::new());
        };
        self.search_vector(query, provider.as_ref(), limit).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use tempfile::TempDir;

    #[derive(Debug)]
    struct MockEmbeddingProvider;

    #[async_trait]
    impl EmbeddingProvider for MockEmbeddingProvider {
        fn name(&self) -> &str {
            "mock"
        }

        fn dimension(&self) -> i32 {
            128
        }

        async fn embed(&self, _text: &str) -> Result<Vec<f32>> {
            Ok(vec![0.1; 128])
        }

        async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![0.1; 128]).collect())
        }
    }

    #[tokio::test]
    async fn test_hybrid_searcher_creation() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(LanceDbStore::open(dir.path()).await.unwrap());

        let searcher = HybridSearcher::new(store.clone());
        assert!(!searcher.has_vector_search());

        let provider = Arc::new(MockEmbeddingProvider);
        let searcher = HybridSearcher::with_embeddings(store, provider);
        assert!(searcher.has_vector_search());
    }

    #[tokio::test]
    async fn test_search_empty_store() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(LanceDbStore::open(dir.path()).await.unwrap());

        let searcher = HybridSearcher::new(store);
        let results = searcher.search("test", 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_reranker_enabled_by_config() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(LanceDbStore::open(dir.path()).await.unwrap());

        // Default: no reranker
        let searcher = HybridSearcher::new(store.clone());
        assert!(!searcher.has_reranker());

        // Enable via config
        let config = RerankerConfig::default();
        assert!(config.enabled); // Enabled by default
        let searcher = HybridSearcher::new(store.clone()).with_reranker_config(&config);
        assert!(searcher.has_reranker());

        // Disable via config
        let mut config = RerankerConfig::default();
        config.enabled = false;
        let searcher = HybridSearcher::new(store).with_reranker_config(&config);
        assert!(!searcher.has_reranker());
    }

    #[tokio::test]
    async fn test_reranker_with_default() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(LanceDbStore::open(dir.path()).await.unwrap());

        let searcher = HybridSearcher::new(store).with_reranker();
        assert!(searcher.has_reranker());
    }

    #[tokio::test]
    async fn test_reranker_without() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(LanceDbStore::open(dir.path()).await.unwrap());

        let searcher = HybridSearcher::new(store)
            .with_reranker()
            .without_reranker();
        assert!(!searcher.has_reranker());
    }
}
