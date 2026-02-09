//! Embedding API types.

use serde::Deserialize;
use serde::Serialize;

// ============================================================================
// Encoding format
// ============================================================================

/// Encoding format for embedding vectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EncodingFormat {
    /// Return embeddings as float array (default).
    #[default]
    Float,
    /// Return embeddings as base64 encoded string.
    Base64,
}

// ============================================================================
// Request parameters
// ============================================================================

/// Input for embedding creation - single text or multiple texts.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum EmbeddingInput {
    /// Single text input.
    Single(String),
    /// Multiple text inputs.
    Multiple(Vec<String>),
}

impl From<String> for EmbeddingInput {
    fn from(s: String) -> Self {
        EmbeddingInput::Single(s)
    }
}

impl From<&str> for EmbeddingInput {
    fn from(s: &str) -> Self {
        EmbeddingInput::Single(s.to_string())
    }
}

impl From<Vec<String>> for EmbeddingInput {
    fn from(v: Vec<String>) -> Self {
        EmbeddingInput::Multiple(v)
    }
}

impl From<Vec<&str>> for EmbeddingInput {
    fn from(v: Vec<&str>) -> Self {
        EmbeddingInput::Multiple(v.into_iter().map(String::from).collect())
    }
}

/// Parameters for creating embeddings.
#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingCreateParams {
    /// Model ID to use (e.g., "text-embedding-3-small").
    pub model: String,

    /// Input text(s) to embed.
    pub input: EmbeddingInput,

    /// Encoding format for the embeddings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding_format: Option<EncodingFormat>,

    /// Number of dimensions for the output (for models that support it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<i32>,

    /// Optional user identifier for abuse monitoring.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

impl EmbeddingCreateParams {
    /// Create new embedding parameters with required fields.
    pub fn new(model: impl Into<String>, input: impl Into<EmbeddingInput>) -> Self {
        Self {
            model: model.into(),
            input: input.into(),
            encoding_format: None,
            dimensions: None,
            user: None,
        }
    }

    /// Set encoding format.
    pub fn encoding_format(mut self, format: EncodingFormat) -> Self {
        self.encoding_format = Some(format);
        self
    }

    /// Set dimensions (for models that support variable dimensions like text-embedding-3-*).
    pub fn dimensions(mut self, dims: i32) -> Self {
        self.dimensions = Some(dims);
        self
    }

    /// Set user identifier.
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }
}

// ============================================================================
// Response types
// ============================================================================

/// Token usage for embedding requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingUsage {
    /// Number of tokens in the prompt.
    pub prompt_tokens: i32,

    /// Total number of tokens used.
    pub total_tokens: i32,
}

/// Individual embedding result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Embedding {
    /// The embedding vector.
    pub embedding: Vec<f64>,

    /// Index of this embedding in the input list.
    pub index: i32,

    /// Object type (always "embedding").
    pub object: String,
}

/// Response from the embedding API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEmbeddingResponse {
    /// Object type (always "list").
    pub object: String,

    /// Model used.
    pub model: String,

    /// List of embedding results.
    pub data: Vec<Embedding>,

    /// Token usage information.
    pub usage: EmbeddingUsage,
}

impl CreateEmbeddingResponse {
    /// Get the first embedding vector (convenience for single input).
    pub fn embedding(&self) -> Option<&[f64]> {
        self.data.first().map(|e| e.embedding.as_slice())
    }

    /// Get all embedding vectors.
    pub fn embeddings(&self) -> Vec<&[f64]> {
        self.data.iter().map(|e| e.embedding.as_slice()).collect()
    }

    /// Get the number of dimensions in the embedding vectors.
    pub fn dimensions(&self) -> Option<usize> {
        self.data.first().map(|e| e.embedding.len())
    }
}

#[cfg(test)]
#[path = "embeddings.test.rs"]
mod tests;
