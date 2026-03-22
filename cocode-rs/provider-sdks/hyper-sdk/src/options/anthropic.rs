//! Anthropic-specific options.

use super::ProviderMarker;
use super::ProviderOptionsData;
use super::TypedProviderOptions;
use serde::Deserialize;
use serde::Serialize;
use std::any::Any;
use std::collections::HashMap;

/// Cache control type for Anthropic.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheControl {
    /// Ephemeral cache (default).
    #[default]
    Ephemeral,
}

/// Anthropic-specific options.
#[derive(Debug, Clone, Default)]
pub struct AnthropicOptions {
    /// Budget in tokens for extended thinking.
    pub thinking_budget_tokens: Option<i32>,
    /// Cache control for prompt caching.
    pub cache_control: Option<CacheControl>,
    /// Metadata to include with the request.
    pub metadata: Option<AnthropicMetadata>,
    /// Arbitrary extra parameters passed through to the API request body.
    #[doc(hidden)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Metadata for Anthropic requests.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicMetadata {
    /// User ID for tracking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

impl AnthropicOptions {
    /// Create new Anthropic options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set thinking budget in tokens.
    pub fn with_thinking_budget(mut self, tokens: i32) -> Self {
        self.thinking_budget_tokens = Some(tokens);
        self
    }

    /// Set cache control.
    pub fn with_cache_control(mut self, control: CacheControl) -> Self {
        self.cache_control = Some(control);
        self
    }

    /// Set user ID metadata.
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.metadata = Some(AnthropicMetadata {
            user_id: Some(user_id.into()),
        });
        self
    }

    /// Convert to boxed ProviderOptions.
    pub fn boxed(self) -> Box<dyn ProviderOptionsData> {
        Box::new(self)
    }
}

impl ProviderMarker for AnthropicOptions {
    const PROVIDER_NAME: &'static str = "anthropic";
}

impl TypedProviderOptions for AnthropicOptions {}

impl ProviderOptionsData for AnthropicOptions {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn ProviderOptionsData> {
        Box::new(self.clone())
    }

    fn provider_name(&self) -> Option<&'static str> {
        Some(Self::PROVIDER_NAME)
    }
}

#[cfg(test)]
#[path = "anthropic.test.rs"]
mod tests;
