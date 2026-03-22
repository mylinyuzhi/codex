//! Embedding types for vector generation.

use crate::response::TokenUsage;
use serde::Deserialize;
use serde::Serialize;

/// Request for generating embeddings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedRequest {
    /// Input texts to embed.
    pub input: Vec<String>,
    /// Optional dimensions for the embedding (if model supports it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<i32>,
    /// Encoding format (default is float).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding_format: Option<EncodingFormat>,
}

/// Encoding format for embeddings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EncodingFormat {
    /// Float32 values.
    #[default]
    Float,
    /// Base64-encoded bytes.
    Base64,
}

impl EmbedRequest {
    /// Create a request for a single text.
    pub fn single(text: impl Into<String>) -> Self {
        Self {
            input: vec![text.into()],
            dimensions: None,
            encoding_format: None,
        }
    }

    /// Create a request for multiple texts.
    pub fn batch(texts: Vec<String>) -> Self {
        Self {
            input: texts,
            dimensions: None,
            encoding_format: None,
        }
    }

    /// Set the embedding dimensions.
    pub fn dimensions(mut self, dims: i32) -> Self {
        self.dimensions = Some(dims);
        self
    }

    /// Set the encoding format.
    pub fn encoding_format(mut self, format: EncodingFormat) -> Self {
        self.encoding_format = Some(format);
        self
    }
}

/// A single embedding result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Embedding {
    /// Index of this embedding in the batch.
    pub index: i64,
    /// The embedding vector.
    pub embedding: Vec<f32>,
}

impl Embedding {
    /// Create a new embedding.
    pub fn new(index: i64, embedding: Vec<f32>) -> Self {
        Self { index, embedding }
    }

    /// Get the dimensionality of the embedding.
    pub fn dimensions(&self) -> usize {
        self.embedding.len()
    }

    /// Calculate cosine similarity with another embedding.
    pub fn cosine_similarity(&self, other: &Embedding) -> f32 {
        if self.embedding.len() != other.embedding.len() {
            return 0.0;
        }

        let dot: f32 = self
            .embedding
            .iter()
            .zip(other.embedding.iter())
            .map(|(a, b)| a * b)
            .sum();

        let norm_a: f32 = self.embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = other.embedding.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }
}

/// Response from embedding generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedResponse {
    /// The generated embeddings.
    pub embeddings: Vec<Embedding>,
    /// Model used for embedding.
    pub model: String,
    /// Token usage statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
}

impl EmbedResponse {
    /// Create a new embed response.
    pub fn new(model: impl Into<String>, embeddings: Vec<Embedding>) -> Self {
        Self {
            embeddings,
            model: model.into(),
            usage: None,
        }
    }

    /// Set token usage.
    pub fn with_usage(mut self, usage: TokenUsage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Get the first embedding (for single-input requests).
    pub fn first(&self) -> Option<&Embedding> {
        self.embeddings.first()
    }

    /// Get embedding by index.
    pub fn get(&self, index: usize) -> Option<&Embedding> {
        self.embeddings.get(index)
    }
}

#[cfg(test)]
#[path = "embedding.test.rs"]
mod tests;
