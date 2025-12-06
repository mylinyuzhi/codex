//! High-level retrieval service.
//!
//! Provides a unified API for code retrieval with feature flags.
//!
//! ## Configuration
//!
//! Retrieval has its own independent configuration system:
//! - Project-level: `.codex/retrieval.toml`
//! - Global: `~/.codex/retrieval.toml`
//!
//! ## Usage
//!
//! ```ignore
//! use codex_retrieval::RetrievalService;
//!
//! // Get service for current working directory (loads config automatically)
//! let service = RetrievalService::for_workdir(&cwd).await?;
//! let results = service.search("function definition").await?;
//! ```

use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use codex_utils_cache::BlockingLruCache;
use once_cell::sync::Lazy;
use tokio::sync::RwLock;

use crate::chunking::CodeChunkerService;
use crate::chunking::supported_languages_info;
use crate::config::RetrievalConfig;
use crate::error::Result;
use crate::error::RetrievalErr;
use crate::query::rewriter::QueryRewriter;
use crate::query::rewriter::RewrittenQuery;
use crate::query::rewriter::SimpleRewriter;
use crate::search::HybridSearcher;
use crate::search::RecentFilesCache;
use crate::storage::lancedb::LanceDbStore;
use crate::traits::EmbeddingProvider;
use crate::types::CodeChunk;
use crate::types::SearchResult;

/// Maximum number of cached RetrievalService instances.
/// Prevents unbounded memory growth in long-running processes.
const MAX_CACHED_SERVICES: usize = 16;

/// Global service instance cache by workdir with LRU eviction.
static INSTANCES: Lazy<BlockingLruCache<PathBuf, Arc<RetrievalService>>> = Lazy::new(|| {
    BlockingLruCache::new(NonZeroUsize::new(MAX_CACHED_SERVICES).expect("capacity > 0"))
});

/// Feature flags for retrieval system.
#[derive(Debug, Clone, Copy, Default)]
pub struct RetrievalFeatures {
    /// Enable BM25 full-text search (basic code search).
    pub code_search: bool,
    /// Enable vector similarity search.
    pub vector_search: bool,
    /// Enable query rewriting (CN/EN translation, expansion).
    pub query_rewrite: bool,
}

impl RetrievalFeatures {
    /// Create with all features disabled.
    pub fn none() -> Self {
        Self::default()
    }

    /// Create with code search enabled.
    pub fn with_code_search() -> Self {
        Self {
            code_search: true,
            ..Default::default()
        }
    }

    /// Enable all features.
    pub fn all() -> Self {
        Self {
            code_search: true,
            vector_search: true,
            query_rewrite: true,
        }
    }

    /// Check if any search feature is enabled.
    pub fn has_search(&self) -> bool {
        self.code_search || self.vector_search
    }
}

/// Default capacity for recent files cache.
const DEFAULT_RECENT_FILES_CAPACITY: usize = 50;

/// High-level retrieval service.
///
/// Integrates search, query rewriting, and embedding providers.
pub struct RetrievalService {
    config: RetrievalConfig,
    features: RetrievalFeatures,
    searcher: HybridSearcher,
    rewriter: Arc<dyn QueryRewriter>,
    /// LRU cache for recently accessed files (temporal relevance signal).
    recent_files: RwLock<RecentFilesCache>,
}

impl RetrievalService {
    /// Get or create a RetrievalService for the given working directory.
    ///
    /// Loads configuration from retrieval.toml files:
    /// 1. `{workdir}/.codex/retrieval.toml` (project-level)
    /// 2. `~/.codex/retrieval.toml` (global)
    ///
    /// Returns `NotEnabled` error if retrieval is not configured/enabled.
    /// Instances are cached by canonicalized workdir path.
    pub async fn for_workdir(workdir: &Path) -> Result<Arc<Self>> {
        // Canonicalize path for cache key
        let canonical = workdir
            .canonicalize()
            .unwrap_or_else(|_| workdir.to_path_buf());

        // Try to get from cache (LRU cache with bounded capacity)
        if let Some(service) = INSTANCES.get(&canonical) {
            return Ok(service);
        }

        // Load config
        let config = RetrievalConfig::load(workdir)?;

        // Check if enabled
        if !config.enabled {
            return Err(RetrievalErr::NotEnabled);
        }

        // Create new service with default features
        let features = RetrievalFeatures {
            code_search: true,
            query_rewrite: true,
            ..Default::default()
        };

        let service = Arc::new(Self::new(config, features).await?);

        // Cache the instance (LRU eviction handles memory bounds)
        INSTANCES.insert(canonical, Arc::clone(&service));

        tracing::info!(
            workdir = ?workdir,
            languages = %supported_languages_info(),
            "RetrievalService initialized"
        );
        Ok(service)
    }

    /// Check if retrieval is configured (without initializing).
    pub fn is_configured(workdir: &Path) -> bool {
        RetrievalConfig::load(workdir)
            .map(|c| c.enabled)
            .unwrap_or(false)
    }

    /// Create a new retrieval service with BM25-only search.
    pub async fn new(config: RetrievalConfig, features: RetrievalFeatures) -> Result<Self> {
        let store = Arc::new(LanceDbStore::open(&config.data_dir).await?);
        let max_chunks_per_file = config.search.max_chunks_per_file as usize;
        let searcher = HybridSearcher::new(store)
            .with_max_chunks_per_file(max_chunks_per_file)
            .with_reranker_config(&config.reranker);
        let rewriter: Arc<dyn QueryRewriter> = Arc::new(SimpleRewriter::new());
        let recent_files = RwLock::new(RecentFilesCache::new(DEFAULT_RECENT_FILES_CAPACITY));

        Ok(Self {
            config,
            features,
            searcher,
            rewriter,
            recent_files,
        })
    }

    /// Create with an embedding provider for vector search.
    pub async fn with_embeddings(
        config: RetrievalConfig,
        features: RetrievalFeatures,
        provider: Arc<dyn EmbeddingProvider>,
    ) -> Result<Self> {
        let store = Arc::new(LanceDbStore::open(&config.data_dir).await?);
        let max_chunks_per_file = config.search.max_chunks_per_file as usize;
        let searcher = HybridSearcher::with_embeddings(store, provider)
            .with_max_chunks_per_file(max_chunks_per_file)
            .with_reranker_config(&config.reranker);
        let rewriter: Arc<dyn QueryRewriter> = Arc::new(SimpleRewriter::new().with_expansion(true));
        let recent_files = RwLock::new(RecentFilesCache::new(DEFAULT_RECENT_FILES_CAPACITY));

        Ok(Self {
            config,
            features,
            searcher,
            rewriter,
            recent_files,
        })
    }

    /// Set a custom query rewriter.
    pub fn with_rewriter(mut self, rewriter: Arc<dyn QueryRewriter>) -> Self {
        self.rewriter = rewriter;
        self
    }

    /// Search for code matching the query.
    ///
    /// Applies query rewriting if enabled, then performs hybrid search.
    ///
    /// # Arguments
    /// * `query` - Search query string
    /// * `limit` - Maximum number of results (if None, uses config.search.n_final)
    pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>> {
        self.search_with_limit(query, None).await
    }

    /// Search with explicit limit parameter.
    pub async fn search_with_limit(
        &self,
        query: &str,
        limit: Option<i32>,
    ) -> Result<Vec<SearchResult>> {
        if !self.features.has_search() {
            return Ok(Vec::new());
        }

        // Apply query rewriting if enabled
        let effective_query = if self.features.query_rewrite {
            let rewritten = self.rewriter.rewrite(query).await?;
            tracing::debug!(
                original = %query,
                rewritten = %rewritten.rewritten,
                translated = rewritten.was_translated,
                "Query rewritten"
            );
            rewritten.effective_query()
        } else {
            query.to_string()
        };

        // Perform search
        let limit = limit.unwrap_or(self.config.search.n_final);
        self.searcher.search(&effective_query, limit).await
    }

    /// Search using BM25 full-text search only.
    ///
    /// Unlike `search()`, this bypasses vector search and RRF fusion.
    pub async fn search_bm25(&self, query: &str, limit: i32) -> Result<Vec<SearchResult>> {
        if !self.features.code_search {
            return Ok(Vec::new());
        }

        // Apply query rewriting if enabled
        let effective_query = if self.features.query_rewrite {
            self.rewriter.rewrite(query).await?.effective_query()
        } else {
            query.to_string()
        };

        self.searcher.search_bm25(&effective_query, limit).await
    }

    /// Search using vector similarity only.
    ///
    /// Returns empty results if embeddings are not configured.
    pub async fn search_vector(&self, query: &str, limit: i32) -> Result<Vec<SearchResult>> {
        if !self.has_vector_search() {
            return Ok(Vec::new());
        }

        // Apply query rewriting if enabled
        let effective_query = if self.features.query_rewrite {
            self.rewriter.rewrite(query).await?.effective_query()
        } else {
            query.to_string()
        };

        self.searcher.search_vector_only(&effective_query, limit).await
    }

    /// Rewrite a query without searching.
    ///
    /// Returns None if query rewriting is disabled.
    pub async fn rewrite_query(&self, query: &str) -> Option<Result<RewrittenQuery>> {
        if self.features.query_rewrite {
            Some(self.rewriter.rewrite(query).await)
        } else {
            None
        }
    }

    /// Get current features.
    pub fn features(&self) -> &RetrievalFeatures {
        &self.features
    }

    /// Get configuration.
    pub fn config(&self) -> &RetrievalConfig {
        &self.config
    }

    /// Check if vector search is available.
    pub fn has_vector_search(&self) -> bool {
        self.features.vector_search && self.searcher.has_vector_search()
    }

    // ========== Recent Files API ==========

    /// Notify that a file has been accessed or edited.
    ///
    /// This updates the LRU cache for temporal relevance in search results.
    /// Recently accessed files will be boosted in search ranking.
    ///
    /// # Arguments
    /// * `path` - File path (absolute or relative to workspace)
    /// * `chunks` - Optional pre-computed chunks. If None, file will be
    ///              read and chunked automatically.
    ///
    /// # Example
    /// ```ignore
    /// // With auto-chunking (reads file content)
    /// service.notify_file_accessed(Path::new("src/main.rs"), None).await?;
    ///
    /// // With pre-computed chunks
    /// service.notify_file_accessed(path, Some(chunks)).await?;
    /// ```
    pub async fn notify_file_accessed(
        &self,
        path: &Path,
        chunks: Option<Vec<CodeChunk>>,
    ) -> Result<()> {
        let chunks = match chunks {
            Some(c) => c,
            None => self.chunk_file(path).await?,
        };
        self.recent_files
            .write()
            .await
            .notify_file_accessed(path, chunks);
        Ok(())
    }

    /// Remove a file from the recent files cache.
    ///
    /// Call this when a file is closed or deleted.
    pub async fn remove_recent_file(&self, path: &Path) {
        self.recent_files.write().await.remove(path);
    }

    /// Clear all recent files from the cache.
    pub async fn clear_recent_files(&self) {
        self.recent_files.write().await.clear();
    }

    /// Check if a file is in the recent files cache.
    pub async fn is_recent_file(&self, path: &Path) -> bool {
        self.recent_files.read().await.contains(path)
    }

    /// Get the number of files in the recent files cache.
    pub async fn recent_files_count(&self) -> usize {
        self.recent_files.read().await.len()
    }

    /// Internal: read and chunk a file.
    ///
    /// Returns empty vec if file is not readable or not a supported language.
    async fn chunk_file(&self, path: &Path) -> Result<Vec<CodeChunk>> {
        // Read file content
        let content = match tokio::fs::read_to_string(path).await {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!(path = ?path, error = %e, "Failed to read file for chunking");
                return Ok(Vec::new());
            }
        };

        // Get file extension for language detection
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("txt");

        // Get filepath string
        let filepath_str = path.to_string_lossy().to_string();

        // Create chunker and chunk content
        let max_chunk_size = self.config.chunking.max_chunk_size as usize;
        let chunker = CodeChunkerService::new(max_chunk_size);

        let spans = match chunker.chunk(&content, extension) {
            Ok(s) => s,
            Err(e) => {
                tracing::debug!(path = ?path, error = %e, "Failed to chunk file");
                return Ok(Vec::new());
            }
        };

        // Convert spans to CodeChunks
        let workspace = "recent"; // Mark as from recent files
        let chunks: Vec<CodeChunk> = spans
            .into_iter()
            .enumerate()
            .map(|(i, span)| CodeChunk {
                id: format!("{}:{}:{}", workspace, filepath_str, i),
                source_id: workspace.to_string(),
                filepath: filepath_str.clone(),
                language: extension.to_string(),
                content: span.content,
                start_line: span.start_line,
                end_line: span.end_line,
                embedding: None,
                modified_time: None,
                workspace: workspace.to_string(),
                branch: None,
                content_hash: String::new(),
                indexed_at: 0,
            })
            .collect();

        Ok(chunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_service_creation() {
        let dir = TempDir::new().unwrap();
        let mut config = RetrievalConfig::default();
        config.data_dir = dir.path().to_path_buf();

        let features = RetrievalFeatures::with_code_search();
        let service = RetrievalService::new(config, features).await.unwrap();

        assert!(service.features().code_search);
        assert!(!service.features().vector_search);
    }

    #[tokio::test]
    async fn test_search_disabled_returns_empty() {
        let dir = TempDir::new().unwrap();
        let mut config = RetrievalConfig::default();
        config.data_dir = dir.path().to_path_buf();

        let features = RetrievalFeatures::none();
        let service = RetrievalService::new(config, features).await.unwrap();

        let results = service.search("test query").await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_rewrite_query_disabled_returns_none() {
        let dir = TempDir::new().unwrap();
        let mut config = RetrievalConfig::default();
        config.data_dir = dir.path().to_path_buf();

        let features = RetrievalFeatures::with_code_search();
        let service = RetrievalService::new(config, features).await.unwrap();

        assert!(service.rewrite_query("test").await.is_none());
    }

    #[tokio::test]
    async fn test_rewrite_query_enabled() {
        let dir = TempDir::new().unwrap();
        let mut config = RetrievalConfig::default();
        config.data_dir = dir.path().to_path_buf();

        let features = RetrievalFeatures {
            code_search: true,
            query_rewrite: true,
            ..Default::default()
        };
        let service = RetrievalService::new(config, features).await.unwrap();

        let result = service.rewrite_query("test function").await;
        assert!(result.is_some());
        let rewritten = result.unwrap().unwrap();
        assert_eq!(rewritten.original, "test function");
    }

    #[test]
    fn test_features_none() {
        let features = RetrievalFeatures::none();
        assert!(!features.code_search);
        assert!(!features.vector_search);
        assert!(!features.query_rewrite);
        assert!(!features.has_search());
    }

    #[test]
    fn test_features_all() {
        let features = RetrievalFeatures::all();
        assert!(features.code_search);
        assert!(features.vector_search);
        assert!(features.query_rewrite);
        assert!(features.has_search());
    }

    // ========== Recent Files Tests ==========

    #[tokio::test]
    async fn test_recent_files_empty_on_creation() {
        let dir = TempDir::new().unwrap();
        let mut config = RetrievalConfig::default();
        config.data_dir = dir.path().to_path_buf();

        let features = RetrievalFeatures::with_code_search();
        let service = RetrievalService::new(config, features).await.unwrap();

        assert_eq!(service.recent_files_count().await, 0);
    }

    #[tokio::test]
    async fn test_notify_file_accessed_with_chunks() {
        let dir = TempDir::new().unwrap();
        let mut config = RetrievalConfig::default();
        config.data_dir = dir.path().to_path_buf();

        let features = RetrievalFeatures::with_code_search();
        let service = RetrievalService::new(config, features).await.unwrap();

        let path = Path::new("src/main.rs");
        let chunks = vec![CodeChunk {
            id: "test:main.rs:0".to_string(),
            source_id: "test".to_string(),
            filepath: "src/main.rs".to_string(),
            language: "rust".to_string(),
            content: "fn main() {}".to_string(),
            start_line: 1,
            end_line: 1,
            embedding: None,
            modified_time: None,
            workspace: "test".to_string(),
            branch: None,
            content_hash: String::new(),
            indexed_at: 0,
        }];

        service
            .notify_file_accessed(path, Some(chunks))
            .await
            .unwrap();

        assert!(service.is_recent_file(path).await);
        assert_eq!(service.recent_files_count().await, 1);
    }

    #[tokio::test]
    async fn test_notify_file_accessed_auto_chunk() {
        let dir = TempDir::new().unwrap();
        let mut config = RetrievalConfig::default();
        config.data_dir = dir.path().to_path_buf();

        let features = RetrievalFeatures::with_code_search();
        let service = RetrievalService::new(config, features).await.unwrap();

        // Create a temporary file
        let file_path = dir.path().join("test.rs");
        std::fs::write(&file_path, "fn main() {\n    println!(\"hello\");\n}").unwrap();

        // Notify with None to trigger auto-chunking
        service
            .notify_file_accessed(&file_path, None)
            .await
            .unwrap();

        assert!(service.is_recent_file(&file_path).await);
        assert_eq!(service.recent_files_count().await, 1);
    }

    #[tokio::test]
    async fn test_remove_recent_file() {
        let dir = TempDir::new().unwrap();
        let mut config = RetrievalConfig::default();
        config.data_dir = dir.path().to_path_buf();

        let features = RetrievalFeatures::with_code_search();
        let service = RetrievalService::new(config, features).await.unwrap();

        let path = Path::new("src/main.rs");
        service
            .notify_file_accessed(path, Some(vec![]))
            .await
            .unwrap();
        assert!(service.is_recent_file(path).await);

        service.remove_recent_file(path).await;
        assert!(!service.is_recent_file(path).await);
    }

    #[tokio::test]
    async fn test_clear_recent_files() {
        let dir = TempDir::new().unwrap();
        let mut config = RetrievalConfig::default();
        config.data_dir = dir.path().to_path_buf();

        let features = RetrievalFeatures::with_code_search();
        let service = RetrievalService::new(config, features).await.unwrap();

        service
            .notify_file_accessed(Path::new("a.rs"), Some(vec![]))
            .await
            .unwrap();
        service
            .notify_file_accessed(Path::new("b.rs"), Some(vec![]))
            .await
            .unwrap();
        assert_eq!(service.recent_files_count().await, 2);

        service.clear_recent_files().await;
        assert_eq!(service.recent_files_count().await, 0);
    }

    #[tokio::test]
    async fn test_notify_nonexistent_file_returns_empty_chunks() {
        let dir = TempDir::new().unwrap();
        let mut config = RetrievalConfig::default();
        config.data_dir = dir.path().to_path_buf();

        let features = RetrievalFeatures::with_code_search();
        let service = RetrievalService::new(config, features).await.unwrap();

        // Notify with non-existent file (auto-chunk should return empty)
        let path = Path::new("/nonexistent/file.rs");
        service.notify_file_accessed(path, None).await.unwrap();

        // File should still be tracked, just with empty chunks
        assert!(service.is_recent_file(path).await);
    }
}
