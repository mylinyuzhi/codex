//! Language model trait (V4).
//!
//! This module defines the `LanguageModelV4` trait for implementing language models
//! that follow the Vercel AI SDK v4 specification.

use async_trait::async_trait;
use regex::Regex;
use std::collections::HashMap;

use super::LanguageModelV4CallOptions;
use super::LanguageModelV4GenerateResult;
use super::LanguageModelV4StreamResult;
use crate::errors::AISdkError;

/// The language model trait (V4).
///
/// This trait defines the interface for language models following the
/// Vercel AI SDK v4 specification.
#[async_trait]
pub trait LanguageModelV4: Send + Sync {
    /// Get the specification version.
    fn specification_version(&self) -> &'static str {
        "v4"
    }

    /// Get the provider name.
    fn provider(&self) -> &str;

    /// Get the model ID.
    fn model_id(&self) -> &str;

    /// Get the supported URL patterns for this model.
    ///
    /// Returns a map of URL schemes to regex patterns that match supported URLs.
    fn supported_urls(&self) -> HashMap<String, Vec<Regex>> {
        HashMap::new()
    }

    /// Generate a response.
    async fn do_generate(
        &self,
        options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4GenerateResult, AISdkError>;

    /// Generate a streaming response.
    async fn do_stream(
        &self,
        options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4StreamResult, AISdkError>;
}

#[cfg(test)]
#[path = "language_model_v4.test.rs"]
mod tests;
