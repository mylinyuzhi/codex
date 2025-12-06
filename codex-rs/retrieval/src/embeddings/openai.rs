//! OpenAI embeddings provider.
//!
//! Uses the OpenAI Embeddings API with text-embedding-3-small model.

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;

use crate::config::default_embedding_dimension;
use crate::error::Result;
use crate::error::RetrievalErr;
use crate::traits::EmbeddingProvider;

/// Default model for embeddings.
const DEFAULT_MODEL: &str = "text-embedding-3-small";
/// Default API base URL.
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// OpenAI embeddings provider.
#[derive(Debug, Clone)]
pub struct OpenAIEmbeddings {
    api_key: String,
    model: String,
    dimension: i32,
    base_url: String,
    client: reqwest::Client,
}

impl OpenAIEmbeddings {
    /// Create a new OpenAI embeddings provider.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: DEFAULT_MODEL.to_string(),
            dimension: default_embedding_dimension(),
            base_url: DEFAULT_BASE_URL.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Set the model to use.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Set the embedding dimension.
    ///
    /// For text-embedding-3-small, valid values are 256, 512, 1024, 1536.
    pub fn with_dimension(mut self, dimension: i32) -> Self {
        self.dimension = dimension;
        self
    }

    /// Set the base URL for API requests.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Make an embedding request to the API.
    async fn request_embeddings(&self, input: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let url = format!("{}/embeddings", self.base_url);

        let request = EmbeddingRequest {
            model: self.model.clone(),
            input,
            dimensions: Some(self.dimension),
            encoding_format: Some("float".to_string()),
        };

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| RetrievalErr::EmbeddingFailed {
                cause: e.to_string(),
            })?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(RetrievalErr::EmbeddingFailed {
                cause: format!("API error {status}: {error_text}"),
            });
        }

        let result: EmbeddingResponse =
            response
                .json()
                .await
                .map_err(|e| RetrievalErr::EmbeddingFailed {
                    cause: e.to_string(),
                })?;

        // Sort by index to ensure correct order
        let mut embeddings: Vec<(i32, Vec<f32>)> = result
            .data
            .into_iter()
            .map(|e| (e.index, e.embedding))
            .collect();
        embeddings.sort_by_key(|(idx, _)| *idx);

        Ok(embeddings.into_iter().map(|(_, e)| e).collect())
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAIEmbeddings {
    fn dimension(&self) -> i32 {
        self.dimension
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let results = self.request_embeddings(vec![text.to_string()]).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| RetrievalErr::EmbeddingFailed {
                cause: "Empty response".to_string(),
            })
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        self.request_embeddings(texts.to_vec()).await
    }
}

#[derive(Debug, Serialize)]
struct EmbeddingRequest {
    model: String,
    input: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encoding_format: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
    #[allow(dead_code)]
    model: String,
    #[allow(dead_code)]
    usage: EmbeddingUsage,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    index: i32,
    embedding: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingUsage {
    #[allow(dead_code)]
    prompt_tokens: i32,
    #[allow(dead_code)]
    total_tokens: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let provider = OpenAIEmbeddings::new("test-key");
        assert_eq!(provider.dimension(), default_embedding_dimension());
        assert_eq!(provider.model, DEFAULT_MODEL);
    }

    #[test]
    fn test_with_dimension() {
        let provider = OpenAIEmbeddings::new("test-key").with_dimension(512);
        assert_eq!(provider.dimension(), 512);
    }

    #[test]
    fn test_with_model() {
        let provider = OpenAIEmbeddings::new("test-key").with_model("text-embedding-3-large");
        assert_eq!(provider.model, "text-embedding-3-large");
    }

    #[test]
    fn test_with_base_url() {
        let provider = OpenAIEmbeddings::new("test-key").with_base_url("https://custom.api.com");
        assert_eq!(provider.base_url, "https://custom.api.com");
    }
}
