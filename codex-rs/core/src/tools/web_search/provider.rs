//! WebSearch provider trait and common types.

use crate::error::Result as CodexResult;
use crate::model_family::ModelFamily;
use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;

/// A single search result from a web search provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchResult {
    /// The title of the search result
    pub title: String,
    /// A snippet or description of the result
    pub snippet: String,
    /// The URL of the result
    pub url: String,
}

/// Trait for web search providers.
///
/// Implementors provide different backend search engines (DuckDuckGo, Tavily, etc.)
/// with a unified interface.
#[async_trait]
pub trait WebSearchProvider: Send + Sync {
    /// Execute a web search with the given query.
    ///
    /// # Arguments
    /// - `query`: The search query string
    /// - `max_results`: Maximum number of results to return
    ///
    /// # Returns
    /// A list of search results, limited to `max_results` items.
    async fn search(&self, query: &str, max_results: usize) -> CodexResult<Vec<SearchResult>>;

    /// Get the name of this provider.
    fn name(&self) -> &str;

    /// Check if this provider is currently available.
    ///
    /// For example, API-based providers may check for API key presence.
    async fn is_available(&self) -> bool;

    /// Check if this provider is compatible with the given model family.
    ///
    /// For example, OpenAI's native web_search tool only works with GPT models.
    fn is_compatible(&self, model_family: &ModelFamily) -> bool;
}

/// Format search results for display to the LLM.
pub fn format_results_for_llm(results: &[SearchResult], provider_name: &str) -> String {
    if results.is_empty() {
        return format!("No search results found using {}.", provider_name);
    }

    let mut output = format!(
        "Found {} search result{} using {}:\n\n",
        results.len(),
        if results.len() == 1 { "" } else { "s" },
        provider_name
    );

    for (index, result) in results.iter().enumerate() {
        output.push_str(&format!("{}. **{}**\n", index + 1, result.title));
        output.push_str(&format!("   {}\n", result.snippet));
        output.push_str(&format!("   URL: {}\n\n", result.url));
    }

    output.push_str(
        "You can reference these results to provide current, accurate information to the user.",
    );
    output
}
