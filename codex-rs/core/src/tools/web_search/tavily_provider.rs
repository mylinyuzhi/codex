//! Tavily web search provider implementation.
//!
//! Uses Tavily's AI-optimized search API. Requires TAVILY_API_KEY environment variable.

use super::provider::{SearchResult, WebSearchProvider};
use crate::error::{CodexErr, Result as CodexResult};
use crate::model_family::ModelFamily;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::env;

/// Tavily search provider.
///
/// Uses Tavily's API for AI-optimized search results.
/// Requires `TAVILY_API_KEY` environment variable.
pub struct TavilyProvider {
    client: reqwest::Client,
    api_key: Option<String>,
}

impl TavilyProvider {
    /// Create a new Tavily provider, reading API key from environment.
    pub fn new() -> Self {
        let api_key = env::var("TAVILY_API_KEY").ok();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { client, api_key }
    }

    /// Execute search via Tavily API.
    async fn fetch_results(
        &self,
        query: &str,
        max_results: usize,
    ) -> CodexResult<Vec<SearchResult>> {
        let api_key = self.api_key.as_ref().ok_or_else(|| {
            CodexErr::Fatal(
                "Tavily provider requires TAVILY_API_KEY environment variable".to_string(),
            )
        })?;

        let request_body = TavilyRequest {
            query: query.to_string(),
            max_results: max_results.min(20) as i32, // Tavily limits to 20
        };

        let response = self
            .client
            .post("https://api.tavily.com/search")
            .header("Content-Type", "application/json")
            .bearer_auth(api_key)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| CodexErr::Fatal(format!("Tavily request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(CodexErr::Fatal(format!(
                "Tavily API returned status {}: {}",
                status, body
            )));
        }

        let tavily_response: TavilyResponse = response
            .json()
            .await
            .map_err(|e| CodexErr::Fatal(format!("Failed to parse Tavily response: {}", e)))?;

        Ok(tavily_response
            .results
            .into_iter()
            .map(|r| SearchResult {
                title: r.title,
                snippet: r.content,
                url: r.url,
            })
            .collect())
    }
}

impl Default for TavilyProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WebSearchProvider for TavilyProvider {
    async fn search(&self, query: &str, max_results: usize) -> CodexResult<Vec<SearchResult>> {
        self.fetch_results(query, max_results).await
    }

    fn name(&self) -> &str {
        "Tavily"
    }

    async fn is_available(&self) -> bool {
        self.api_key.is_some()
    }

    fn is_compatible(&self, _model_family: &ModelFamily) -> bool {
        // Compatible with all model families
        true
    }
}

#[derive(Debug, Serialize)]
struct TavilyRequest {
    query: String,
    max_results: i32,
}

#[derive(Debug, Deserialize)]
struct TavilyResponse {
    results: Vec<TavilyResult>,
}

#[derive(Debug, Deserialize)]
struct TavilyResult {
    title: String,
    url: String,
    content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tavily_provider_creation() {
        let provider = TavilyProvider::new();
        assert_eq!(provider.name(), "Tavily");
    }

    #[tokio::test]
    async fn test_is_available_without_key() {
        // Temporarily clear the environment variable
        let original = env::var("TAVILY_API_KEY").ok();
        unsafe {
            env::remove_var("TAVILY_API_KEY");
        }

        let provider = TavilyProvider::new();
        assert!(!provider.is_available().await);

        // Restore original value if it existed
        if let Some(key) = original {
            unsafe {
                env::set_var("TAVILY_API_KEY", key);
            }
        }
    }
}
