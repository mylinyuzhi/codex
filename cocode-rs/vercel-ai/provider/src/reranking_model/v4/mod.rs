//! Reranking model trait and related types (V4).
//!
//! This module defines the `RerankingModelV4` trait for implementing reranking models
//! that follow the Vercel AI SDK v4 specification.

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

use crate::errors::AISdkError;
use crate::shared::ProviderOptions;

/// The reranking model trait (V4).
///
/// This trait defines the interface for reranking models following the
/// Vercel AI SDK v4 specification.
#[async_trait]
pub trait RerankingModelV4: Send + Sync {
    /// Get the specification version.
    fn specification_version(&self) -> &'static str {
        "v4"
    }

    /// Get the provider name.
    fn provider(&self) -> &str;

    /// Get the model ID.
    fn model_id(&self) -> &str;

    /// Rerank documents based on a query.
    async fn do_rerank(
        &self,
        options: RerankingModelV4CallOptions,
    ) -> Result<RerankingModelV4Result, AISdkError>;
}

/// Options for a reranking model call.
#[derive(Debug, Clone, Default)]
pub struct RerankingModelV4CallOptions {
    /// The query to rank documents against.
    pub query: String,
    /// The documents to rank.
    pub documents: Vec<String>,
    /// The number of top results to return.
    pub top_n: Option<usize>,
    /// Whether to return documents in the result.
    pub return_documents: Option<bool>,
    /// Provider-specific options.
    pub provider_options: Option<ProviderOptions>,
    /// Abort signal for cancellation.
    pub abort_signal: Option<CancellationToken>,
    /// Headers to include in the request.
    pub headers: Option<HashMap<String, String>>,
}

impl RerankingModelV4CallOptions {
    /// Create new call options.
    pub fn new(query: impl Into<String>, documents: Vec<String>) -> Self {
        Self {
            query: query.into(),
            documents,
            ..Default::default()
        }
    }

    /// Set the number of top results.
    pub fn with_top_n(mut self, top_n: usize) -> Self {
        self.top_n = Some(top_n);
        self
    }

    /// Set whether to return documents.
    pub fn with_return_documents(mut self, return_documents: bool) -> Self {
        self.return_documents = Some(return_documents);
        self
    }

    /// Set provider options.
    pub fn with_provider_options(mut self, options: ProviderOptions) -> Self {
        self.provider_options = Some(options);
        self
    }

    /// Set the abort signal.
    pub fn with_abort_signal(mut self, signal: CancellationToken) -> Self {
        self.abort_signal = Some(signal);
        self
    }
}

/// The result of a reranking call.
#[derive(Debug, Clone)]
pub struct RerankingModelV4Result {
    /// The reranked documents.
    pub results: Vec<RerankedDocument>,
    /// Token usage (if available).
    pub usage: Option<RerankingUsage>,
}

impl RerankingModelV4Result {
    /// Create a new reranking result.
    pub fn new(results: Vec<RerankedDocument>) -> Self {
        Self {
            results,
            usage: None,
        }
    }

    /// Set the usage.
    pub fn with_usage(mut self, usage: RerankingUsage) -> Self {
        self.usage = Some(usage);
        self
    }
}

/// A reranked document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RerankedDocument {
    /// The index of the document in the original list.
    pub index: usize,
    /// The relevance score (0-1).
    pub relevance_score: f32,
    /// The document text (if return_documents was true).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
}

impl RerankedDocument {
    /// Create a new reranked document.
    pub fn new(index: usize, relevance_score: f32) -> Self {
        Self {
            index,
            relevance_score,
            document: None,
        }
    }

    /// Set the document text.
    pub fn with_document(mut self, document: impl Into<String>) -> Self {
        self.document = Some(document.into());
        self
    }
}

/// Token usage for reranking calls.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RerankingUsage {
    /// The number of tokens in the prompt.
    pub prompt_tokens: u64,
    /// The total number of tokens.
    pub total_tokens: u64,
}

impl RerankingUsage {
    /// Create new reranking usage.
    pub fn new(prompt_tokens: u64) -> Self {
        Self {
            prompt_tokens,
            total_tokens: prompt_tokens,
        }
    }
}

#[cfg(test)]
#[path = "reranking_model_v4.test.rs"]
mod tests;
