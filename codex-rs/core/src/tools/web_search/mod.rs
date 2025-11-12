//! Web search tool implementations.
//!
//! Provides pluggable web search backends including DuckDuckGo, Tavily, and OpenAI.

mod duckduckgo_provider;
mod openai_provider;
mod provider;
mod tavily_provider;

pub use duckduckgo_provider::DuckDuckGoProvider;
pub use openai_provider::OpenAIProvider;
pub use provider::SearchResult;
pub use provider::WebSearchProvider;
pub use provider::format_results_for_llm;
pub use tavily_provider::TavilyProvider;

use crate::error::CodexErr;
use crate::error::Result as CodexResult;
use crate::model_family::ModelFamily;
use codex_protocol::config_types::WebSearchConfig;
use codex_protocol::config_types::WebSearchProvider as WebSearchProviderConfig;
use std::sync::Arc;

/// Select the appropriate web search provider based on configuration and model family.
///
/// # Provider Selection Logic
///
/// - **DuckDuckGo**: Always uses DuckDuckGo provider (no fallback needed)
/// - **Tavily**: Uses Tavily if API key is available, otherwise returns error
/// - **OpenAI**: Uses OpenAI native search if model is compatible (GPT series),
///   otherwise falls back to DuckDuckGo with a warning
///
/// # Arguments
///
/// - `config`: The configured provider from user settings
/// - `model_family`: The current model family being used
///
/// # Returns
///
/// An Arc-wrapped provider ready for use, or an error if the provider cannot be initialized.
pub async fn select_provider(
    config: WebSearchProviderConfig,
    model_family: &ModelFamily,
) -> CodexResult<Arc<dyn WebSearchProvider>> {
    let provider: Arc<dyn WebSearchProvider> = match config {
        WebSearchProviderConfig::DuckDuckGo => {
            let provider = DuckDuckGoProvider::new();
            Arc::new(provider)
        }
        WebSearchProviderConfig::Tavily => {
            let provider = TavilyProvider::new();
            if !provider.is_available().await {
                return Err(CodexErr::Fatal(
                    "Tavily provider requires TAVILY_API_KEY environment variable. \
                     Get your free API key at https://tavily.com/"
                        .to_string(),
                ));
            }
            Arc::new(provider)
        }
        WebSearchProviderConfig::OpenAI => {
            let provider = OpenAIProvider::new();
            if !provider.is_compatible(model_family) {
                tracing::warn!(
                    "OpenAI web search is not available for model family '{}'. \
                     Falling back to DuckDuckGo provider.",
                    model_family.slug
                );
                Arc::new(DuckDuckGoProvider::new())
            } else {
                Arc::new(provider)
            }
        }
    };

    Ok(provider)
}

/// Create a web search provider synchronously based on configuration.
///
/// This is a simpler version that doesn't perform async availability checks.
/// Tavily API key presence is checked, but actual network availability
/// is verified at search time.
pub fn create_provider_sync(
    config: &WebSearchConfig,
    model_family: &ModelFamily,
) -> Arc<dyn WebSearchProvider> {
    match config.provider {
        WebSearchProviderConfig::DuckDuckGo => Arc::new(DuckDuckGoProvider::new()),
        WebSearchProviderConfig::Tavily => {
            let provider = TavilyProvider::new();
            // Check for API key presence (synchronous)
            if std::env::var("TAVILY_API_KEY").is_err() {
                tracing::warn!(
                    "TAVILY_API_KEY not found, falling back to DuckDuckGo. \
                     Get a free key at https://tavily.com/"
                );
                Arc::new(DuckDuckGoProvider::new())
            } else {
                Arc::new(provider)
            }
        }
        WebSearchProviderConfig::OpenAI => {
            let provider = OpenAIProvider::new();
            if !provider.is_compatible(model_family) {
                tracing::warn!(
                    "OpenAI web search is not compatible with model '{}'. \
                     Falling back to DuckDuckGo provider.",
                    model_family.slug
                );
                Arc::new(DuckDuckGoProvider::new())
            } else {
                Arc::new(provider)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_family::find_family_for_model;

    #[test]
    fn test_create_duckduckgo_provider_sync() {
        let model_family = find_family_for_model("gpt-5-codex")
            .expect("gpt-5-codex should be a valid model family");
        let config = WebSearchConfig {
            provider: WebSearchProviderConfig::DuckDuckGo,
            max_results: 5,
        };

        let provider = create_provider_sync(&config, &model_family);

        assert_eq!(provider.name(), "DuckDuckGo");
        assert!(provider.is_compatible(&model_family));
    }

    #[test]
    fn test_create_openai_provider_with_gpt_model() {
        let model_family = find_family_for_model("gpt-5-codex")
            .expect("gpt-5-codex should be a valid model family");
        let config = WebSearchConfig {
            provider: WebSearchProviderConfig::OpenAI,
            max_results: 5,
        };

        let provider = create_provider_sync(&config, &model_family);

        assert_eq!(provider.name(), "OpenAI");
    }

    #[test]
    fn test_create_openai_provider_fallback_to_duckduckgo() {
        let model_family = find_family_for_model("codex-mini-latest")
            .expect("codex-mini-latest should be a valid model family");
        let config = WebSearchConfig {
            provider: WebSearchProviderConfig::OpenAI,
            max_results: 5,
        };

        let provider = create_provider_sync(&config, &model_family);

        // Should fallback to DuckDuckGo for non-OpenAI models
        // Note: codex-mini-latest is not a GPT model, so it should fallback
        assert_eq!(provider.name(), "DuckDuckGo");
    }
}
