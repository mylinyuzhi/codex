//! DuckDuckGo web search provider implementation.
//!
//! Uses DuckDuckGo's HTML endpoint with scraping. No API key required.

use super::provider::{SearchResult, WebSearchProvider};
use crate::error::{CodexErr, Result as CodexResult};
use crate::model_family::ModelFamily;
use async_trait::async_trait;
use scraper::{Html, Selector};

/// DuckDuckGo search provider.
///
/// Scrapes results from DuckDuckGo's HTML search interface.
/// No API key or authentication required.
pub struct DuckDuckGoProvider {
    client: reqwest::Client,
}

impl DuckDuckGoProvider {
    /// Create a new DuckDuckGo provider with default configuration.
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { client }
    }

    /// Fetch and parse search results from DuckDuckGo.
    async fn fetch_results(
        &self,
        query: &str,
        max_results: usize,
    ) -> CodexResult<Vec<SearchResult>> {
        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| CodexErr::Fatal(format!("DuckDuckGo request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(CodexErr::Fatal(format!(
                "DuckDuckGo returned status code: {}",
                response.status()
            )));
        }

        let html = response
            .text()
            .await
            .map_err(|e| CodexErr::Fatal(format!("Failed to read DuckDuckGo response: {}", e)))?;

        self.parse_html(&html, max_results)
    }

    /// Parse HTML response and extract search results.
    fn parse_html(&self, html: &str, max_results: usize) -> CodexResult<Vec<SearchResult>> {
        let document = Html::parse_document(html);

        let result_selector = Selector::parse(".result.web-result")
            .map_err(|e| CodexErr::Fatal(format!("Invalid CSS selector: {:?}", e)))?;
        let title_selector = Selector::parse(".result__a")
            .map_err(|e| CodexErr::Fatal(format!("Invalid CSS selector: {:?}", e)))?;
        let snippet_selector = Selector::parse(".result__snippet")
            .map_err(|e| CodexErr::Fatal(format!("Invalid CSS selector: {:?}", e)))?;

        let mut results = Vec::new();

        for result_node in document.select(&result_selector).take(max_results) {
            let title_node = result_node.select(&title_selector).next();
            let snippet_node = result_node.select(&snippet_selector).next();

            if let (Some(title_elem), Some(snippet_elem)) = (title_node, snippet_node) {
                let title = title_elem.text().collect::<String>().trim().to_string();
                let snippet = snippet_elem.text().collect::<String>().trim().to_string();

                // Extract href attribute
                if let Some(href) = title_elem.value().attr("href") {
                    let clean_url = self.clean_duckduckgo_url(href);

                    results.push(SearchResult {
                        title,
                        snippet,
                        url: clean_url,
                    });
                }
            }
        }

        Ok(results)
    }

    /// Clean DuckDuckGo redirect URLs.
    ///
    /// DuckDuckGo uses redirect URLs like:
    /// https://duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com
    ///
    /// This function extracts the actual URL.
    fn clean_duckduckgo_url(&self, url: &str) -> String {
        if url.starts_with("https://duckduckgo.com/l/?uddg=") {
            if let Ok(parsed) = url::Url::parse(url) {
                if let Some(uddg_param) = parsed.query_pairs().find(|(k, _)| k == "uddg") {
                    return uddg_param.1.to_string();
                }
            }
        }
        url.to_string()
    }
}

impl Default for DuckDuckGoProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WebSearchProvider for DuckDuckGoProvider {
    async fn search(&self, query: &str, max_results: usize) -> CodexResult<Vec<SearchResult>> {
        self.fetch_results(query, max_results).await
    }

    fn name(&self) -> &str {
        "DuckDuckGo"
    }

    async fn is_available(&self) -> bool {
        // DuckDuckGo is always available (no API key needed)
        true
    }

    fn is_compatible(&self, _model_family: &ModelFamily) -> bool {
        // Compatible with all model families
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_duckduckgo_url() {
        let provider = DuckDuckGoProvider::new();

        // Test redirect URL
        let redirect = "https://duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fpage";
        let cleaned = provider.clean_duckduckgo_url(redirect);
        assert_eq!(cleaned, "https://example.com/page");

        // Test direct URL
        let direct = "https://example.com/page";
        let cleaned = provider.clean_duckduckgo_url(direct);
        assert_eq!(cleaned, "https://example.com/page");
    }

    #[test]
    fn test_parse_html_empty() {
        let provider = DuckDuckGoProvider::new();
        let html = "<html><body></body></html>";
        let results = provider.parse_html(html, 10).unwrap();
        assert_eq!(results.len(), 0);
    }
}
