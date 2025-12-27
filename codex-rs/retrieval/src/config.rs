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

    /// Reranker configuration (legacy rule-based only)
    #[serde(default)]
    pub reranker: RerankerConfig,

    /// Extended reranker configuration (supports local/remote neural reranking)
    #[serde(default)]
    pub extended_reranker: Option<ExtendedRerankerConfig>,

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
            extended_reranker: None,
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

    /// Maximum total chunks allowed (prevents runaway indexing)
    #[serde(default = "default_max_chunks")]
    pub max_chunks: i64,
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
            max_chunks: default_max_chunks(),
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
        if self.max_chunks <= 0 {
            return Err(RetrievalErr::ConfigError {
                field: "indexing.max_chunks".to_string(),
                cause: format!("must be positive, got {}", self.max_chunks),
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
fn default_max_chunks() -> i64 {
    500_000 // 500k chunks should be sufficient for most projects
}

/// Chunking configuration.
///
/// Uses token-aware tree-sitter CodeSplitter for AST-based chunking.
///
/// Industry research recommends 256-512 tokens for code search:
/// - Too small (<128 tokens): fragments lack context
/// - Too large (>1024 tokens): dilutes embedding, hurts precision
///
/// Reference: LlamaIndex, Milvus RAG guides, Continue's retrieval config.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChunkingConfig {
    /// Maximum tokens per chunk.
    ///
    /// Industry best practice: 256-512 tokens for code search.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: i32,

    /// Token overlap between chunks (~10% of max_tokens).
    ///
    /// Provides semantic continuity across chunk boundaries.
    #[serde(default = "default_overlap_tokens")]
    pub overlap_tokens: i32,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            max_tokens: default_max_tokens(),
            overlap_tokens: default_overlap_tokens(),
        }
    }
}

/// 512 tokens - industry best practice for code search
fn default_max_tokens() -> i32 {
    512
}
/// 50 tokens overlap (~10% of default 512 max_tokens)
fn default_overlap_tokens() -> i32 {
    50
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

// ============================================================================
// Neural Reranker Configuration
// ============================================================================

/// Reranker backend type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RerankerBackend {
    /// Rule-based reranking (default, no ML model required)
    #[default]
    RuleBased,
    /// Local neural reranking using fastembed (ONNX Runtime)
    Local,
    /// Remote API-based reranking (Cohere, VoyageAI, etc.)
    Remote,
    /// Chain multiple rerankers (e.g., rule-based then neural)
    Chain,
}

/// Local neural reranker configuration.
///
/// Uses fastembed-rs with ONNX Runtime for local inference.
/// Models are downloaded on first use and cached locally.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalRerankerConfig {
    /// Model name (e.g., "bge-reranker-base", "jina-reranker-v2")
    #[serde(default = "default_local_model")]
    pub model: String,

    /// Maximum batch size for reranking
    #[serde(default = "default_local_batch_size")]
    pub batch_size: i32,

    /// Model cache directory (defaults to ~/.cache/codex/models)
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,

    /// Show download progress when fetching model
    #[serde(default)]
    pub show_download_progress: bool,
}

impl Default for LocalRerankerConfig {
    fn default() -> Self {
        Self {
            model: default_local_model(),
            batch_size: default_local_batch_size(),
            cache_dir: None,
            show_download_progress: false,
        }
    }
}

fn default_local_model() -> String {
    "bge-reranker-base".to_string()
}

fn default_local_batch_size() -> i32 {
    32
}

/// Remote API reranker provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RerankerProvider {
    /// Cohere Rerank API
    Cohere,
    /// Voyage AI Rerank API
    VoyageAi,
    /// Custom API endpoint (OpenAI-compatible)
    Custom,
}

/// Remote API reranker configuration.
///
/// Supports Cohere, Voyage AI, and custom API endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteRerankerConfig {
    /// API provider
    pub provider: RerankerProvider,

    /// Model name (e.g., "rerank-english-v3.0" for Cohere)
    pub model: String,

    /// Environment variable name containing the API key
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,

    /// Custom API base URL (optional, uses provider default if not set)
    #[serde(default)]
    pub base_url: Option<String>,

    /// Request timeout in seconds
    #[serde(default = "default_remote_timeout")]
    pub timeout_secs: i32,

    /// Maximum retry attempts
    #[serde(default = "default_remote_max_retries")]
    pub max_retries: i32,

    /// Return top-N results (defaults to all)
    #[serde(default)]
    pub top_n: Option<i32>,
}

impl Default for RemoteRerankerConfig {
    fn default() -> Self {
        Self {
            provider: RerankerProvider::Cohere,
            model: "rerank-english-v3.0".to_string(),
            api_key_env: default_api_key_env(),
            base_url: None,
            timeout_secs: default_remote_timeout(),
            max_retries: default_remote_max_retries(),
            top_n: None,
        }
    }
}

fn default_api_key_env() -> String {
    "COHERE_API_KEY".to_string()
}

fn default_remote_timeout() -> i32 {
    10
}

fn default_remote_max_retries() -> i32 {
    2
}

/// Extended reranker configuration with backend selection.
///
/// Supports rule-based, local neural, remote API, and chained rerankers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExtendedRerankerConfig {
    /// Reranker backend type
    #[serde(default)]
    pub backend: RerankerBackend,

    /// Rule-based reranker config (used when backend = "rule_based")
    #[serde(default)]
    pub rule_based: RerankerConfig,

    /// Local neural reranker config (used when backend = "local")
    #[serde(default)]
    pub local: Option<LocalRerankerConfig>,

    /// Remote API reranker config (used when backend = "remote")
    #[serde(default)]
    pub remote: Option<RemoteRerankerConfig>,

    /// Chain of rerankers (used when backend = "chain")
    /// Each entry specifies a backend and its config
    #[serde(default)]
    pub chain: Vec<ChainedRerankerConfig>,
}

impl Default for ExtendedRerankerConfig {
    fn default() -> Self {
        Self {
            backend: RerankerBackend::RuleBased,
            rule_based: RerankerConfig::default(),
            local: None,
            remote: None,
            chain: Vec::new(),
        }
    }
}

/// Configuration for a single reranker in a chain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChainedRerankerConfig {
    /// Backend type for this stage
    pub backend: RerankerBackend,

    /// Rule-based config (if backend = "rule_based")
    #[serde(default)]
    pub rule_based: Option<RerankerConfig>,

    /// Local config (if backend = "local")
    #[serde(default)]
    pub local: Option<LocalRerankerConfig>,

    /// Remote config (if backend = "remote")
    #[serde(default)]
    pub remote: Option<RemoteRerankerConfig>,
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

    /// BM25 k1 parameter (term frequency saturation, code-optimized default)
    ///
    /// NOTE: Not currently used - LanceDB 0.22 doesn't expose BM25 params.
    /// Added for future-proofing when LanceDB supports custom BM25 parameters.
    #[serde(default = "default_bm25_k1")]
    pub bm25_k1: f32,

    /// BM25 b parameter (document length normalization, code-optimized default)
    ///
    /// NOTE: Not currently used - LanceDB 0.22 doesn't expose BM25 params.
    /// Added for future-proofing when LanceDB supports custom BM25 parameters.
    #[serde(default = "default_bm25_b")]
    pub bm25_b: f32,
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
            bm25_k1: default_bm25_k1(),
            bm25_b: default_bm25_b(),
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
fn default_bm25_k1() -> f32 {
    0.8 // Lower than default 1.2, better for code with repeated keywords
}
fn default_bm25_b() -> f32 {
    0.5 // Lower than default 0.75, less length normalization for functions
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

// ============================================================================
// Vector Quantization Configuration
// ============================================================================

/// Vector quantization method for index compression.
///
/// Quantization reduces embedding storage size at the cost of some precision.
/// Use for large indexes (>100k chunks) to reduce memory and improve search speed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum QuantizationMethod {
    /// No quantization (full float32 precision)
    #[default]
    None,
    /// Scalar Quantization (4x compression, <1% recall loss)
    Scalar,
    /// Product Quantization (4-8x compression, 1-3% recall loss)
    Product,
}

/// Vector quantization configuration.
///
/// Applied when creating vector indexes in LanceDB.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuantizationConfig {
    /// Quantization method
    #[serde(default)]
    pub method: QuantizationMethod,

    /// Number of subquantizers for Product Quantization (typically 8-96)
    ///
    /// Higher values = more precision but larger index.
    /// Must divide embedding dimension evenly.
    #[serde(default = "default_pq_num_sub_vectors")]
    pub num_sub_vectors: i32,

    /// Bits per code for Product Quantization (typically 8)
    #[serde(default = "default_pq_num_bits")]
    pub num_bits: i32,
}

impl Default for QuantizationConfig {
    fn default() -> Self {
        Self {
            method: QuantizationMethod::None,
            num_sub_vectors: default_pq_num_sub_vectors(),
            num_bits: default_pq_num_bits(),
        }
    }
}

fn default_pq_num_sub_vectors() -> i32 {
    16 // Good balance for 1536-dim embeddings (1536 / 16 = 96 dims per subvector)
}
fn default_pq_num_bits() -> i32 {
    8 // Standard PQ bits
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

    /// Vector quantization settings (optional)
    ///
    /// When set, applies quantization during vector index creation.
    /// Recommended for large indexes (>100k chunks) to reduce storage.
    #[serde(default)]
    pub quantization: Option<QuantizationConfig>,
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

        // Note: RRF weights (bm25_weight, vector_weight, snippet_weight, recent_weight)
        // are relative importance factors, not probabilities - they don't need to sum to 1.0

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
        if self.chunking.max_tokens <= 0 {
            warnings.push(ConfigWarning::InvalidValue {
                field: "chunking.max_tokens",
                reason: format!("must be > 0, got {}", self.chunking.max_tokens),
            });
        }
        if self.chunking.overlap_tokens < 0 {
            warnings.push(ConfigWarning::InvalidValue {
                field: "chunking.overlap_tokens",
                reason: format!("must be >= 0, got {}", self.chunking.overlap_tokens),
            });
        }
        if self.chunking.overlap_tokens >= self.chunking.max_tokens {
            warnings.push(ConfigWarning::InvalidValue {
                field: "chunking.overlap_tokens",
                reason: format!(
                    "must be < max_tokens ({}), got {}",
                    self.chunking.max_tokens, self.chunking.overlap_tokens
                ),
            });
        }
        // Warn if max_tokens is unusually large (>1024 may hurt search accuracy)
        if self.chunking.max_tokens > 1024 {
            warnings.push(ConfigWarning::InvalidValue {
                field: "chunking.max_tokens",
                reason: format!(
                    "value {} exceeds recommended max (1024). Large chunks may reduce search precision.",
                    self.chunking.max_tokens
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
