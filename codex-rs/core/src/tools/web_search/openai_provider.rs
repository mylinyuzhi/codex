//! OpenAI native web_search provider.
//!
//! This is a marker provider that signals to use OpenAI's native web_search tool type.
//! The actual search is performed by OpenAI's API infrastructure, not locally.

use super::provider::{SearchResult, WebSearchProvider};
use crate::error::Result as CodexResult;
use crate::model_family::ModelFamily;
use async_trait::async_trait;

/// OpenAI native web search provider.
///
/// This provider doesn't actually perform searches locally. Instead, it's a marker
/// that tells the tool system to use OpenAI's native `web_search` tool type.
///
/// Only compatible with OpenAI GPT models.
pub struct OpenAIProvider;

impl OpenAIProvider {
    /// Create a new OpenAI provider.
    pub fn new() -> Self {
        Self
    }
}

impl Default for OpenAIProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WebSearchProvider for OpenAIProvider {
    async fn search(&self, _query: &str, _max_results: usize) -> CodexResult<Vec<SearchResult>> {
        // OpenAI provider doesn't execute searches locally.
        // The actual search is performed by OpenAI's API when it sees the
        // ToolSpec::WebSearch {} tool in the conversation.
        //
        // Return empty results since this method should never be called.
        Ok(vec![])
    }

    fn name(&self) -> &str {
        "OpenAI"
    }

    async fn is_available(&self) -> bool {
        // Always available - the actual availability is determined by API access
        true
    }

    fn is_compatible(&self, model_family: &ModelFamily) -> bool {
        // Only compatible with OpenAI GPT models (check by model slug/family name)
        let slug = &model_family.slug;
        slug.starts_with("gpt-") || slug.starts_with("o3") || slug.starts_with("o4")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_family::find_family_for_model;

    #[tokio::test]
    async fn test_openai_provider() {
        let provider = OpenAIProvider::new();
        assert_eq!(provider.name(), "OpenAI");
        assert!(provider.is_available().await);

        // Test empty search results
        let results = provider.search("test", 5).await.unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_is_compatible() {
        let provider = OpenAIProvider::new();

        // Test with GPT model family
        let openai_family = find_family_for_model("gpt-5-codex")
            .expect("gpt-5-codex should be a valid model family");
        assert!(provider.is_compatible(&openai_family));

        // Test with non-GPT model family
        let other_family = find_family_for_model("o3").expect("o3 should be a valid model family");
        assert!(provider.is_compatible(&other_family)); // o3 is OpenAI model
    }
}
