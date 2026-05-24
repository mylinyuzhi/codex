//! Web search configuration types.

use serde::Deserialize;
use serde::Serialize;

/// Web search provider backend selection.
#[derive(Debug, Serialize, Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WebSearchProvider {
    /// DuckDuckGo HTML scraping (free, no API key required)
    #[default]
    DuckDuckGo,
    /// Tavily AI-optimized search API (requires TAVILY_API_KEY)
    Tavily,
    /// OpenAI native web_search tool (only for GPT models)
    OpenAI,
}

/// Web search configuration.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct WebSearchConfig {
    /// Search provider backend
    #[serde(default)]
    pub provider: WebSearchProvider,
    /// Maximum number of search results to return (1-20)
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    /// API key for Tavily provider (falls back to TAVILY_API_KEY env var)
    #[serde(default)]
    pub api_key: Option<String>,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            provider: WebSearchProvider::default(),
            max_results: default_max_results(),
            api_key: None,
        }
    }
}

fn default_max_results() -> usize {
    5
}
