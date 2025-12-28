//! Hybrid search combining BM25 and vector search.
//!
//! Uses RRF (Reciprocal Rank Fusion) to combine results from
//! multiple search methods.
//!
//! Optionally applies rule-based reranking for improved relevance.
//!
//! BM25 search can use either:
//! - Custom BM25 index with tunable k1/b parameters (recommended for code)
//! - LanceDB built-in FTS (fallback)

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use crate::config::ExtendedRerankerConfig;
use crate::config::RerankerConfig;
use crate::error::Result;
use crate::reranker::Reranker;
use crate::reranker::RuleBasedReranker;
use crate::reranker::RuleBasedRerankerConfig;
use crate::reranker::create_reranker;
use crate::search::Bm25Searcher;
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
use crate::types::ChunkRef;
use crate::types::CodeChunk;
use crate::types::ScoreType;
use crate::types::SearchQuery;
use crate::types::SearchResult;

/// Hybrid searcher combining BM25 and vector search.
pub struct HybridSearcher {
    store: Arc<LanceDbStore>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    config: RrfConfig,
    /// Maximum chunks per file (0 = unlimited)
    max_chunks_per_file: usize,
    /// Optional reranker for post-retrieval score adjustment
    reranker: Option<Arc<dyn Reranker>>,
    /// Optional snippet searcher for symbol-based search
    snippet_searcher: Option<SnippetSearcher>,
    /// Workspace root for hydrating content from files
    workspace_root: Option<PathBuf>,
    /// Custom BM25 searcher with tunable k1/b parameters.
    /// If set, uses this instead of LanceDB FTS for better code search.
    bm25_searcher: Option<Arc<Bm25Searcher>>,
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
            workspace_root: None,
            bm25_searcher: None,
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
            workspace_root: None,
            bm25_searcher: None,
        }
    }

    /// Set custom BM25 searcher with tunable k1/b parameters.
    ///
    /// When set, uses the custom BM25 index instead of LanceDB FTS.
    /// This provides better code search quality with optimized parameters:
    /// - k1 = 0.8 (reduced keyword repetition weight)
    /// - b = 0.5 (reduced length normalization)
    pub fn with_bm25_searcher(mut self, searcher: Arc<Bm25Searcher>) -> Self {
        self.bm25_searcher = Some(searcher);
        self
    }

    /// Check if custom BM25 search is enabled.
    pub fn has_custom_bm25(&self) -> bool {
        self.bm25_searcher.is_some()
    }

    /// Set workspace root for hydrating content from files.
    ///
    /// When set, `search_hydrated` will read fresh content from files
    /// instead of returning the indexed content.
    pub fn with_workspace_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.workspace_root = Some(root.into());
        self
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
            self.reranker = Some(Arc::new(RuleBasedReranker::with_config(reranker_config)));
        } else {
            self.reranker = None;
        }
        self
    }

    /// Enable reranking with extended configuration (supports local/remote backends).
    ///
    /// Returns error if the reranker could not be created.
    pub fn with_extended_reranker_config(
        mut self,
        config: &ExtendedRerankerConfig,
    ) -> Result<Self> {
        self.reranker = Some(create_reranker(config)?);
        Ok(self)
    }

    /// Enable reranking with a custom reranker.
    pub fn with_custom_reranker(mut self, reranker: Arc<dyn Reranker>) -> Self {
        self.reranker = Some(reranker);
        self
    }

    /// Enable reranking with default configuration.
    pub fn with_reranker(mut self) -> Self {
        self.reranker = Some(Arc::new(RuleBasedReranker::new()));
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
        self.search_with_recent(query, limit, &[]).await
    }

    /// Search with recently accessed files for temporal relevance boost.
    ///
    /// Like `search`, but also includes `recent_results` in RRF fusion.
    /// Recent results get a 20% weight boost by default (configurable in RrfConfig).
    pub async fn search_with_recent(
        &self,
        query: &str,
        limit: i32,
        recent_results: &[SearchResult],
    ) -> Result<Vec<SearchResult>> {
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
            return self.apply_reranking(query, limited).await;
        }

        // Use fuse_all_results for multi-source fusion
        let fused = fuse_all_results(
            &bm25_results,
            &vector_results,
            &snippet_results,
            recent_results,
            &config,
            limit,
        );

        // Deduplicate overlapping chunks
        let deduped = deduplicate_results(fused);
        // Apply per-file limit for diversity
        let limited = self.apply_per_file_limit(deduped);
        // Apply reranking if enabled
        self.apply_reranking(query, limited).await
    }

    /// Search with content hydration from files.
    ///
    /// Like `search`, but reads fresh content from the file system instead of
    /// returning the indexed content. This ensures results always reflect the
    /// current file state.
    ///
    /// Requires `workspace_root` to be set via `with_workspace_root`.
    /// If not set, falls back to regular `search`.
    ///
    /// Files that have been deleted or cannot be read are skipped with a warning.
    pub async fn search_hydrated(&self, query: &str, limit: i32) -> Result<Vec<SearchResult>> {
        self.search_hydrated_with_recent(query, limit, &[]).await
    }

    /// Search with content hydration and recently accessed files boost.
    ///
    /// Combines `search_with_recent` and hydration for full search flow:
    /// 1. Runs hybrid search with recent_results in RRF fusion
    /// 2. Hydrates results by reading fresh content from files
    ///
    /// Use this method when you have recently accessed files to boost in ranking.
    pub async fn search_hydrated_with_recent(
        &self,
        query: &str,
        limit: i32,
        recent_results: &[SearchResult],
    ) -> Result<Vec<SearchResult>> {
        let results = self
            .search_with_recent(query, limit, recent_results)
            .await?;

        let Some(ref workspace_root) = self.workspace_root else {
            // No workspace root set, return results as-is
            return Ok(results);
        };

        // Hydrate each result by reading fresh content from file
        self.hydrate_results(results, workspace_root)
    }

    /// Hydrate search results by reading fresh content from files.
    ///
    /// Converts each result's content to the current file content.
    /// Files that don't exist or can't be read are skipped.
    /// Sets `is_stale` field to indicate if content was modified since indexing.
    fn hydrate_results(
        &self,
        results: Vec<SearchResult>,
        workspace_root: &Path,
    ) -> Result<Vec<SearchResult>> {
        let mut hydrated = Vec::with_capacity(results.len());

        for result in results {
            match self.hydrate_chunk(&result.chunk, workspace_root) {
                Ok((chunk, is_stale)) => {
                    hydrated.push(SearchResult {
                        chunk,
                        score: result.score,
                        score_type: result.score_type,
                        is_stale: Some(is_stale),
                    });
                }
                Err(e) => {
                    // Hydration failed (file moved/deleted) - fall back to indexed content
                    tracing::warn!(
                        filepath = %result.chunk.filepath,
                        error = %e,
                        "Hydration failed, using indexed content"
                    );
                    hydrated.push(SearchResult {
                        chunk: result.chunk,
                        score: result.score,
                        score_type: result.score_type,
                        is_stale: Some(true), // Mark as stale since we couldn't verify
                    });
                }
            }
        }

        Ok(hydrated)
    }

    /// Hydrate a single chunk by reading fresh content from file.
    ///
    /// Returns (hydrated_chunk, is_stale) where is_stale indicates if the
    /// file was modified since indexing.
    fn hydrate_chunk(
        &self,
        chunk: &CodeChunk,
        workspace_root: &Path,
    ) -> std::io::Result<(CodeChunk, bool)> {
        // Create ChunkRef from CodeChunk
        let chunk_ref = ChunkRef::from(chunk);

        // Read fresh content using ChunkRef's read_content method
        let hydrated = chunk_ref.read_content(workspace_root)?;
        let is_stale = !hydrated.is_fresh;

        // Log if content is stale (hash mismatch)
        if is_stale {
            tracing::debug!(
                filepath = %chunk.filepath,
                "Chunk content differs from index, returning fresh content"
            );
        }

        // Return updated CodeChunk with fresh content
        let hydrated_chunk = CodeChunk {
            content: hydrated.content,
            ..chunk.clone()
        };

        Ok((hydrated_chunk, is_stale))
    }

    /// Apply reranking if enabled.
    async fn apply_reranking(
        &self,
        query: &str,
        mut results: Vec<SearchResult>,
    ) -> Result<Vec<SearchResult>> {
        if let Some(ref reranker) = self.reranker {
            reranker.rerank(query, &mut results).await?;
        }
        Ok(results)
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
    ///
    /// Uses custom BM25 searcher if available (with tunable k1/b parameters),
    /// otherwise falls back to LanceDB FTS.
    pub async fn search_bm25(&self, query: &str, limit: i32) -> Result<Vec<SearchResult>> {
        // Use custom BM25 searcher if available
        if let Some(ref bm25) = self.bm25_searcher {
            let search_query = SearchQuery {
                text: query.to_string(),
                limit,
                ..Default::default()
            };
            return bm25.search(&search_query).await;
        }

        // Fall back to LanceDB FTS
        let chunks = self.store.search_fts(query, limit).await?;

        // Convert to SearchResult with rank-based scores
        Ok(chunks
            .into_iter()
            .enumerate()
            .map(|(i, chunk)| SearchResult {
                chunk,
                score: 1.0 / (i as f32 + 1.0), // Simple rank-based score
                score_type: ScoreType::Bm25,
                is_stale: None, // Not hydrated yet
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
                is_stale: None, // Not hydrated yet
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

    #[tokio::test]
    async fn test_with_workspace_root() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(LanceDbStore::open(dir.path()).await.unwrap());

        let searcher = HybridSearcher::new(store.clone());
        assert!(searcher.workspace_root.is_none());

        let searcher = HybridSearcher::new(store).with_workspace_root(dir.path());
        assert!(searcher.workspace_root.is_some());
        assert_eq!(searcher.workspace_root.as_ref().unwrap(), dir.path());
    }

    #[tokio::test]
    async fn test_search_hydrated_without_workspace_root() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(LanceDbStore::open(dir.path()).await.unwrap());

        // Without workspace_root, search_hydrated should work but not hydrate
        let searcher = HybridSearcher::new(store);
        let results = searcher.search_hydrated("test", 10).await.unwrap();
        assert!(results.is_empty()); // Empty store
    }

    #[tokio::test]
    async fn test_hydrate_chunk_basic() {
        use std::io::Write;

        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.rs");
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "fn main() {{").unwrap();
        writeln!(file, "    println!(\"hello\");").unwrap();
        writeln!(file, "}}").unwrap();

        let store_dir = TempDir::new().unwrap();
        let store = Arc::new(LanceDbStore::open(store_dir.path()).await.unwrap());

        let searcher = HybridSearcher::new(store).with_workspace_root(dir.path());

        // Create a chunk that matches the file
        let chunk = CodeChunk {
            id: "test:test.rs:0".to_string(),
            source_id: "test".to_string(),
            filepath: "test.rs".to_string(),
            language: "rust".to_string(),
            content: "old content".to_string(), // This should be replaced
            start_line: 1,
            end_line: 3,
            embedding: None,
            modified_time: None,
            workspace: "test".to_string(),
            content_hash: String::new(),
            indexed_at: 0,
            parent_symbol: None,
            is_overview: false,
        };

        let (hydrated_chunk, is_stale) = searcher.hydrate_chunk(&chunk, dir.path()).unwrap();
        assert!(hydrated_chunk.content.contains("fn main()"));
        assert!(hydrated_chunk.content.contains("println!"));
        assert!(!hydrated_chunk.content.contains("old content"));
        // No hash in the original chunk, so is_stale should be false (treated as fresh)
        assert!(!is_stale);
    }

    #[tokio::test]
    async fn test_hydrate_chunk_file_not_found() {
        let dir = TempDir::new().unwrap();
        let store_dir = TempDir::new().unwrap();
        let store = Arc::new(LanceDbStore::open(store_dir.path()).await.unwrap());

        let searcher = HybridSearcher::new(store).with_workspace_root(dir.path());

        // Create a chunk referencing a non-existent file
        let chunk = CodeChunk {
            id: "test:nonexistent.rs:0".to_string(),
            source_id: "test".to_string(),
            filepath: "nonexistent.rs".to_string(),
            language: "rust".to_string(),
            content: "old content".to_string(),
            start_line: 1,
            end_line: 1,
            embedding: None,
            modified_time: None,
            workspace: "test".to_string(),
            content_hash: String::new(),
            indexed_at: 0,
            parent_symbol: None,
            is_overview: false,
        };

        let result = searcher.hydrate_chunk(&chunk, dir.path());
        assert!(result.is_err()); // Should fail for non-existent file
    }
}
