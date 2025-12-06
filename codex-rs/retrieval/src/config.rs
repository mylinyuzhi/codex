//! Configuration for the retrieval system.

use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;

/// Main retrieval configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetrievalConfig {
    /// Whether retrieval is enabled
    #[serde(default)]
    pub enabled: bool,

    /// Directory for storing index data
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,

    /// Indexing configuration
    #[serde(default)]
    pub indexing: IndexingConfig,

    /// Chunking configuration
    #[serde(default)]
    pub chunking: ChunkingConfig,

    /// Search configuration
    #[serde(default)]
    pub search: SearchConfig,

    /// Reranker configuration
    #[serde(default)]
    pub reranker: RerankerConfig,

    /// Embedding configuration (optional, for vector search)
    #[serde(default)]
    pub embedding: Option<EmbeddingConfig>,

    /// Query rewrite configuration (optional, for LLM-based query enhancement)
    #[serde(default)]
    pub query_rewrite: Option<QueryRewriteConfig>,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            data_dir: default_data_dir(),
            indexing: IndexingConfig::default(),
            chunking: ChunkingConfig::default(),
            search: SearchConfig::default(),
            reranker: RerankerConfig::default(),
            embedding: None,
            query_rewrite: None,
        }
    }
}

fn default_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
        .join("retrieval")
}

/// Indexing configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IndexingConfig {
    /// Maximum file size in MB to index
    #[serde(default = "default_max_file_size_mb")]
    pub max_file_size_mb: i32,

    /// Number of files to process in a batch
    #[serde(default = "default_batch_size")]
    pub batch_size: i32,

    /// Commit interval (every N operations)
    #[serde(default = "default_commit_interval")]
    pub commit_interval: i32,

    /// Lock timeout in seconds
    #[serde(default = "default_lock_timeout_secs")]
    pub lock_timeout_secs: i32,

    /// Enable file watching on startup (default: false)
    #[serde(default)]
    pub watch_enabled: bool,

    /// Watch debounce interval in milliseconds (default: 500)
    #[serde(default = "default_watch_debounce_ms")]
    pub watch_debounce_ms: i32,
}

impl Default for IndexingConfig {
    fn default() -> Self {
        Self {
            max_file_size_mb: default_max_file_size_mb(),
            batch_size: default_batch_size(),
            commit_interval: default_commit_interval(),
            lock_timeout_secs: default_lock_timeout_secs(),
            watch_enabled: false,
            watch_debounce_ms: default_watch_debounce_ms(),
        }
    }
}

impl IndexingConfig {
    /// Validate configuration values.
    ///
    /// Ensures all numeric values are positive to prevent:
    /// - Integer overflow when casting to unsigned types
    /// - Division by zero errors
    /// - Infinite loops or deadlocks
    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::RetrievalErr;

        if self.max_file_size_mb <= 0 {
            return Err(RetrievalErr::ConfigError {
                field: "indexing.max_file_size_mb".to_string(),
                cause: format!("must be positive, got {}", self.max_file_size_mb),
            });
        }
        if self.batch_size <= 0 {
            return Err(RetrievalErr::ConfigError {
                field: "indexing.batch_size".to_string(),
                cause: format!("must be positive, got {}", self.batch_size),
            });
        }
        if self.commit_interval <= 0 {
            return Err(RetrievalErr::ConfigError {
                field: "indexing.commit_interval".to_string(),
                cause: format!("must be positive, got {}", self.commit_interval),
            });
        }
        if self.lock_timeout_secs <= 0 {
            return Err(RetrievalErr::ConfigError {
                field: "indexing.lock_timeout_secs".to_string(),
                cause: format!("must be positive, got {}", self.lock_timeout_secs),
            });
        }
        if self.watch_debounce_ms <= 0 {
            return Err(RetrievalErr::ConfigError {
                field: "indexing.watch_debounce_ms".to_string(),
                cause: format!("must be positive, got {}", self.watch_debounce_ms),
            });
        }
        Ok(())
    }
}

fn default_max_file_size_mb() -> i32 {
    5
}
fn default_batch_size() -> i32 {
    100
}
fn default_commit_interval() -> i32 {
    100
}
fn default_lock_timeout_secs() -> i32 {
    30
}
fn default_watch_debounce_ms() -> i32 {
    500
}

/// Chunking configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChunkingConfig {
    /// Maximum chunk size in characters
    #[serde(default = "default_max_chunk_size")]
    pub max_chunk_size: i32,

    /// Chunk overlap in characters
    #[serde(default = "default_chunk_overlap")]
    pub chunk_overlap: i32,

    /// Enable smart collapsing for large functions
    #[serde(default = "default_enable_smart_collapse")]
    pub enable_smart_collapse: bool,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            max_chunk_size: default_max_chunk_size(),
            chunk_overlap: default_chunk_overlap(),
            enable_smart_collapse: default_enable_smart_collapse(),
        }
    }
}

fn default_max_chunk_size() -> i32 {
    512
}
fn default_chunk_overlap() -> i32 {
    64
}
fn default_enable_smart_collapse() -> bool {
    true
}

/// Reranker configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RerankerConfig {
    /// Whether reranking is enabled
    #[serde(default = "default_reranker_enabled")]
    pub enabled: bool,

    /// Boost multiplier for exact query term matches in content
    #[serde(default = "default_exact_match_boost")]
    pub exact_match_boost: f32,

    /// Boost multiplier for query terms in file path
    #[serde(default = "default_path_relevance_boost")]
    pub path_relevance_boost: f32,

    /// Boost multiplier for recently modified files
    #[serde(default = "default_recency_boost")]
    pub recency_boost: f32,

    /// Days threshold for recency boost (files modified within this many days get boosted)
    #[serde(default = "default_recency_days_threshold")]
    pub recency_days_threshold: i32,
}

impl Default for RerankerConfig {
    fn default() -> Self {
        Self {
            enabled: default_reranker_enabled(),
            exact_match_boost: default_exact_match_boost(),
            path_relevance_boost: default_path_relevance_boost(),
            recency_boost: default_recency_boost(),
            recency_days_threshold: default_recency_days_threshold(),
        }
    }
}

fn default_reranker_enabled() -> bool {
    true // Enabled by default
}
fn default_exact_match_boost() -> f32 {
    2.0
}
fn default_path_relevance_boost() -> f32 {
    1.5
}
fn default_recency_boost() -> f32 {
    1.2
}
fn default_recency_days_threshold() -> i32 {
    7
}

/// Search configuration.
///
/// Based on Continue's retrieval configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchConfig {
    /// Final number of results to return
    #[serde(default = "default_n_final")]
    pub n_final: i32,

    /// Number of candidates to retrieve initially
    #[serde(default = "default_n_retrieve")]
    pub n_retrieve: i32,

    /// BM25 score threshold (negative, lower = stricter)
    #[serde(default = "default_bm25_threshold")]
    pub bm25_threshold: f32,

    /// Reranking threshold
    #[serde(default = "default_rerank_threshold")]
    pub rerank_threshold: f32,

    /// Enable word stemming
    #[serde(default = "default_enable_stemming")]
    pub enable_stemming: bool,

    /// Enable n-gram generation
    #[serde(default)]
    pub enable_ngrams: bool,

    /// N-gram size
    #[serde(default = "default_ngram_size")]
    pub ngram_size: i32,

    /// BM25 search weight (0.0 - 1.0)
    #[serde(default = "default_bm25_weight")]
    pub bm25_weight: f32,

    /// Vector search weight (0.0 - 1.0)
    #[serde(default = "default_vector_weight")]
    pub vector_weight: f32,

    /// Snippet match weight (0.0 - 1.0)
    #[serde(default = "default_snippet_weight")]
    pub snippet_weight: f32,

    /// Recently edited files weight (0.0 - 1.0)
    ///
    /// Controls how much weight is given to chunks from recently edited files.
    /// Higher values favor recent context over semantic relevance.
    #[serde(default = "default_recent_weight")]
    pub recent_weight: f32,

    /// RRF constant k
    #[serde(default = "default_rrf_k")]
    pub rrf_k: f32,

    /// Path match weight multiplier (from Continue: 10.0)
    #[serde(default = "default_path_weight_multiplier")]
    pub path_weight_multiplier: f32,

    /// Maximum result tokens (from Continue: 8000)
    #[serde(default = "default_max_result_tokens")]
    pub max_result_tokens: i32,

    /// Token truncation strategy
    #[serde(default)]
    pub truncate_strategy: TruncateStrategy,

    /// Maximum token length for filtering (from Tabby: 64)
    #[serde(default = "default_max_token_length")]
    pub max_token_length: i32,

    /// Maximum chunks per file in search results (from Tabby: 2)
    ///
    /// Limits the number of chunks from a single file to ensure result diversity.
    /// This prevents a single highly-relevant file from dominating all results.
    #[serde(default = "default_max_chunks_per_file")]
    pub max_chunks_per_file: i32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            n_final: default_n_final(),
            n_retrieve: default_n_retrieve(),
            bm25_threshold: default_bm25_threshold(),
            rerank_threshold: default_rerank_threshold(),
            enable_stemming: default_enable_stemming(),
            enable_ngrams: false,
            ngram_size: default_ngram_size(),
            bm25_weight: default_bm25_weight(),
            vector_weight: default_vector_weight(),
            snippet_weight: default_snippet_weight(),
            recent_weight: default_recent_weight(),
            rrf_k: default_rrf_k(),
            path_weight_multiplier: default_path_weight_multiplier(),
            max_result_tokens: default_max_result_tokens(),
            truncate_strategy: TruncateStrategy::default(),
            max_token_length: default_max_token_length(),
            max_chunks_per_file: default_max_chunks_per_file(),
        }
    }
}

fn default_n_final() -> i32 {
    20
}
fn default_n_retrieve() -> i32 {
    50
}
fn default_bm25_threshold() -> f32 {
    -2.5
}
fn default_rerank_threshold() -> f32 {
    0.3
}
fn default_enable_stemming() -> bool {
    true
}
fn default_ngram_size() -> i32 {
    3
}
fn default_bm25_weight() -> f32 {
    0.6
}
fn default_vector_weight() -> f32 {
    0.3
}
fn default_snippet_weight() -> f32 {
    0.1
}
fn default_recent_weight() -> f32 {
    0.2 // 20% weight for recently edited files
}
fn default_rrf_k() -> f32 {
    60.0
}
fn default_path_weight_multiplier() -> f32 {
    10.0
}
fn default_max_result_tokens() -> i32 {
    8000
}
fn default_max_token_length() -> i32 {
    64
}
fn default_max_chunks_per_file() -> i32 {
    2
}

/// Token truncation strategy.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TruncateStrategy {
    /// Truncate from the tail
    #[default]
    Tail,
    /// Smart truncation (preserve complete chunks)
    Smart,
}

/// Embedding provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddingConfig {
    /// Provider name (e.g., "openai")
    pub provider: String,

    /// Model name (e.g., "text-embedding-3-small")
    pub model: String,

    /// Embedding dimension
    #[serde(default = "default_embedding_dimension")]
    pub dimension: i32,

    /// API base URL (optional)
    #[serde(default)]
    pub base_url: Option<String>,

    /// Batch size for embedding requests
    #[serde(default = "default_embedding_batch_size")]
    pub batch_size: i32,
}

/// Default embedding dimension (OpenAI text-embedding-3-small).
pub fn default_embedding_dimension() -> i32 {
    1536
}
fn default_embedding_batch_size() -> i32 {
    100
}

/// Query rewrite configuration.
///
/// Enables LLM-based query transformation for improved search results.
/// Includes translation, intent classification, and query expansion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueryRewriteConfig {
    /// Whether query rewriting is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// LLM provider configuration
    #[serde(default)]
    pub llm: LlmConfig,

    /// Cache configuration
    #[serde(default)]
    pub cache: RewriteCacheConfig,

    /// Feature toggles
    #[serde(default)]
    pub features: RewriteFeatures,

    /// Rule-based synonym mappings
    #[serde(default)]
    pub rules: RewriteRules,
}

impl Default for QueryRewriteConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            llm: LlmConfig::default(),
            cache: RewriteCacheConfig::default(),
            features: RewriteFeatures::default(),
            rules: RewriteRules::default(),
        }
    }
}

fn default_true() -> bool {
    true
}

/// LLM provider configuration for query rewriting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmConfig {
    /// Provider name (e.g., "openai", "anthropic")
    #[serde(default = "default_llm_provider")]
    pub provider: String,

    /// Model name (e.g., "gpt-4o-mini")
    #[serde(default = "default_llm_model")]
    pub model: String,

    /// API base URL (optional, for custom endpoints)
    #[serde(default)]
    pub base_url: Option<String>,

    /// Temperature for generation (0.0 - 1.0)
    #[serde(default = "default_llm_temperature")]
    pub temperature: f32,

    /// Maximum tokens for response
    #[serde(default = "default_llm_max_tokens")]
    pub max_tokens: i32,

    /// Request timeout in seconds
    #[serde(default = "default_llm_timeout_secs")]
    pub timeout_secs: i32,

    /// Maximum retry attempts
    #[serde(default = "default_llm_max_retries")]
    pub max_retries: i32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: default_llm_provider(),
            model: default_llm_model(),
            base_url: None,
            temperature: default_llm_temperature(),
            max_tokens: default_llm_max_tokens(),
            timeout_secs: default_llm_timeout_secs(),
            max_retries: default_llm_max_retries(),
        }
    }
}

fn default_llm_provider() -> String {
    "openai".to_string()
}
fn default_llm_model() -> String {
    "gpt-4o-mini".to_string()
}
fn default_llm_temperature() -> f32 {
    0.3
}
fn default_llm_max_tokens() -> i32 {
    500
}
fn default_llm_timeout_secs() -> i32 {
    10
}
fn default_llm_max_retries() -> i32 {
    2
}

/// Cache configuration for query rewriting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RewriteCacheConfig {
    /// Whether caching is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Cache TTL in seconds
    #[serde(default = "default_cache_ttl_secs")]
    pub ttl_secs: i64,

    /// Maximum cache entries
    #[serde(default = "default_cache_max_entries")]
    pub max_entries: i32,
}

impl Default for RewriteCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ttl_secs: default_cache_ttl_secs(),
            max_entries: default_cache_max_entries(),
        }
    }
}

fn default_cache_ttl_secs() -> i64 {
    86400 // 24 hours
}
fn default_cache_max_entries() -> i32 {
    10000
}

/// Feature toggles for query rewriting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RewriteFeatures {
    /// Enable translation (non-English to English)
    #[serde(default = "default_true")]
    pub translation: bool,

    /// Enable intent classification
    #[serde(default = "default_true")]
    pub intent_classification: bool,

    /// Enable query expansion (synonyms, related terms)
    #[serde(default = "default_true")]
    pub expansion: bool,

    /// Enable case variant generation (camelCase, snake_case)
    #[serde(default = "default_true")]
    pub case_variants: bool,
}

impl Default for RewriteFeatures {
    fn default() -> Self {
        Self {
            translation: true,
            intent_classification: true,
            expansion: true,
            case_variants: true,
        }
    }
}

/// Rule-based rewriting configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RewriteRules {
    /// Synonym mappings (term -> [synonyms])
    #[serde(default)]
    pub synonyms: std::collections::HashMap<String, Vec<String>>,
}

impl RewriteRules {
    /// Get default programming synonyms.
    pub fn default_synonyms() -> std::collections::HashMap<String, Vec<String>> {
        let mut map = std::collections::HashMap::new();
        map.insert(
            "function".to_string(),
            vec![
                "fn".to_string(),
                "func".to_string(),
                "method".to_string(),
                "def".to_string(),
            ],
        );
        map.insert(
            "class".to_string(),
            vec![
                "struct".to_string(),
                "type".to_string(),
                "interface".to_string(),
            ],
        );
        map.insert(
            "error".to_string(),
            vec![
                "err".to_string(),
                "exception".to_string(),
                "panic".to_string(),
            ],
        );
        map.insert(
            "authentication".to_string(),
            vec![
                "auth".to_string(),
                "login".to_string(),
                "authorize".to_string(),
            ],
        );
        map.insert(
            "database".to_string(),
            vec![
                "db".to_string(),
                "storage".to_string(),
                "datastore".to_string(),
            ],
        );
        map
    }
}

impl RetrievalConfig {
    /// Load configuration from config files.
    ///
    /// Search order (first found wins):
    /// 1. `{workdir}/.codex/retrieval.toml` (project-level)
    /// 2. `~/.codex/retrieval.toml` (global)
    /// 3. Default (disabled)
    pub fn load(workdir: &std::path::Path) -> crate::error::Result<Self> {
        // Project-level config
        let project_config = workdir.join(".codex/retrieval.toml");
        if project_config.exists() {
            return Self::from_file(&project_config);
        }

        // Global config
        if let Some(home) = dirs::home_dir() {
            let global_config = home.join(".codex/retrieval.toml");
            if global_config.exists() {
                return Self::from_file(&global_config);
            }
        }

        // Return disabled default
        Ok(Self::default())
    }

    /// Load configuration from a specific file.
    pub fn from_file(path: &std::path::Path) -> crate::error::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str(&content).map_err(|e| crate::error::RetrievalErr::ConfigParseError {
            path: path.to_path_buf(),
            cause: e.to_string(),
        })
    }

    /// Validate configuration consistency.
    ///
    /// Returns warnings for potential issues.
    pub fn validate(&self) -> Vec<ConfigWarning> {
        let mut warnings = Vec::new();

        // Check: data_dir should exist
        if !self.data_dir.exists() {
            warnings.push(ConfigWarning::PathNotExists {
                field: "retrieval.data_dir",
                path: self.data_dir.clone(),
            });
        }

        // Check: weights should sum to ~1.0
        let total_weight =
            self.search.bm25_weight + self.search.vector_weight + self.search.snippet_weight;
        if (total_weight - 1.0).abs() > 0.01 {
            warnings.push(ConfigWarning::WeightSumNotOne {
                actual: total_weight,
            });
        }

        // Indexing config validation
        if self.indexing.max_file_size_mb <= 0 {
            warnings.push(ConfigWarning::InvalidValue {
                field: "indexing.max_file_size_mb",
                reason: format!("must be > 0, got {}", self.indexing.max_file_size_mb),
            });
        }
        if self.indexing.batch_size <= 0 {
            warnings.push(ConfigWarning::InvalidValue {
                field: "indexing.batch_size",
                reason: format!("must be > 0, got {}", self.indexing.batch_size),
            });
        }

        // Chunking config validation
        if self.chunking.max_chunk_size <= 0 {
            warnings.push(ConfigWarning::InvalidValue {
                field: "chunking.max_chunk_size",
                reason: format!("must be > 0, got {}", self.chunking.max_chunk_size),
            });
        }
        if self.chunking.chunk_overlap < 0 {
            warnings.push(ConfigWarning::InvalidValue {
                field: "chunking.chunk_overlap",
                reason: format!("must be >= 0, got {}", self.chunking.chunk_overlap),
            });
        }
        if self.chunking.chunk_overlap >= self.chunking.max_chunk_size {
            warnings.push(ConfigWarning::InvalidValue {
                field: "chunking.chunk_overlap",
                reason: format!(
                    "must be < max_chunk_size ({}), got {}",
                    self.chunking.max_chunk_size, self.chunking.chunk_overlap
                ),
            });
        }

        // Search config validation
        if self.search.n_final <= 0 {
            warnings.push(ConfigWarning::InvalidValue {
                field: "search.n_final",
                reason: format!("must be > 0, got {}", self.search.n_final),
            });
        }
        if self.search.n_retrieve < self.search.n_final {
            warnings.push(ConfigWarning::InvalidValue {
                field: "search.n_retrieve",
                reason: format!(
                    "must be >= n_final ({}), got {}",
                    self.search.n_final, self.search.n_retrieve
                ),
            });
        }

        // Embedding config validation
        if let Some(ref embedding) = self.embedding {
            if embedding.dimension <= 0 {
                warnings.push(ConfigWarning::InvalidValue {
                    field: "embedding.dimension",
                    reason: format!("must be > 0, got {}", embedding.dimension),
                });
            }
            if embedding.batch_size <= 0 {
                warnings.push(ConfigWarning::InvalidValue {
                    field: "embedding.batch_size",
                    reason: format!("must be > 0, got {}", embedding.batch_size),
                });
            }
        }

        warnings
    }
}

/// Configuration warning.
#[derive(Debug, Clone)]
pub enum ConfigWarning {
    /// Required dependency missing for a feature
    MissingDependency {
        feature: &'static str,
        required: &'static str,
    },
    /// Path does not exist
    PathNotExists { field: &'static str, path: PathBuf },
    /// Weights don't sum to 1.0
    WeightSumNotOne { actual: f32 },
    /// Embedding dimension mismatch with existing index
    DimensionMismatch { configured: i32, indexed: i32 },
    /// Invalid numeric value
    InvalidValue { field: &'static str, reason: String },
}

impl std::fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigWarning::MissingDependency { feature, required } => {
                write!(
                    f,
                    "Feature '{feature}' requires '{required}' to be configured"
                )
            }
            ConfigWarning::PathNotExists { field, path } => {
                write!(f, "Config '{field}' path does not exist: {path:?}")
            }
            ConfigWarning::WeightSumNotOne { actual } => {
                write!(f, "Search weights sum to {actual:.2}, expected 1.0")
            }
            ConfigWarning::DimensionMismatch {
                configured,
                indexed,
            } => {
                write!(
                    f,
                    "Embedding dimension mismatch: config={configured}, index={indexed}"
                )
            }
            ConfigWarning::InvalidValue { field, reason } => {
                write!(f, "Invalid value for '{field}': {reason}")
            }
        }
    }
}
