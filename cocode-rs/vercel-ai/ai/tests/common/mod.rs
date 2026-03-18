//! Test helpers and macros for vercel-ai integration tests.
//!
//! This module provides utilities for running integration tests against
//! real LLM providers. Tests are gated by environment configuration -
//! if credentials are not provided, tests skip gracefully.

pub mod config;
pub mod fixtures;

pub use config::TestConfig;
pub use config::load_test_config;
pub use fixtures::*;

use std::sync::Arc;

use vercel_ai::LanguageModel;
use vercel_ai::LanguageModelV4;
use vercel_ai::ProviderV4;

/// Create a provider and model from test configuration.
pub fn create_provider_and_model(
    cfg: &TestConfig,
) -> Option<(Arc<dyn ProviderV4>, Arc<dyn LanguageModelV4>)> {
    let provider: Arc<dyn ProviderV4> = match cfg.provider.as_str() {
        "openai" => {
            let settings = vercel_ai_openai::OpenAIProviderSettings {
                api_key: Some(cfg.api_key.clone()),
                base_url: cfg.base_url.clone(),
                ..Default::default()
            };
            Arc::new(vercel_ai_openai::create_openai(settings))
        }
        "anthropic" => {
            let settings = vercel_ai_anthropic::AnthropicProviderSettings {
                api_key: Some(cfg.api_key.clone()),
                base_url: cfg.base_url.clone(),
                ..Default::default()
            };
            Arc::new(vercel_ai_anthropic::create_anthropic(settings))
        }
        "google" => {
            let settings = vercel_ai_google::GoogleGenerativeAIProviderSettings {
                api_key: Some(cfg.api_key.clone()),
                base_url: cfg.base_url.clone(),
                ..Default::default()
            };
            Arc::new(vercel_ai_google::create_google_generative_ai(settings))
        }
        _ => {
            eprintln!("Unknown provider: {}", cfg.provider);
            return None;
        }
    };

    match provider.language_model(&cfg.model) {
        Ok(model) => Some((provider, model)),
        Err(e) => {
            eprintln!("Failed to create model {}: {e}", cfg.model);
            None
        }
    }
}

/// Get a `LanguageModel` enum from a test config, for use with `generate_text` / `stream_text`.
#[allow(dead_code)]
pub fn create_language_model(cfg: &TestConfig) -> Option<LanguageModel> {
    create_provider_and_model(cfg).map(|(_, model)| LanguageModel::from_v4(model))
}

/// Macro to require a provider configuration, skipping the test if not available.
///
/// Optionally checks that a specific capability is enabled for the provider.
///
/// Usage:
/// ```ignore
/// #[tokio::test]
/// async fn test_openai_text_generation() -> anyhow::Result<()> {
///     let (_provider, model) = require_provider!("openai");
///     // Test code...
///     Ok(())
/// }
///
/// #[tokio::test]
/// async fn test_openai_vision() -> anyhow::Result<()> {
///     let (_provider, model) = require_provider!("openai", "vision");
///     // Test code (skips if vision not in CAPABILITIES)...
///     Ok(())
/// }
/// ```
#[macro_export]
macro_rules! require_provider {
    ($provider:expr) => {
        match $crate::common::load_test_config($provider) {
            Some(cfg) if cfg.enabled => match $crate::common::create_provider_and_model(&cfg) {
                Some((provider, model)) => (provider, model),
                None => {
                    eprintln!("Skipping test: failed to create provider '{}'", $provider);
                    return Ok(());
                }
            },
            _ => {
                eprintln!(
                    "Skipping test: provider '{}' not configured in .env",
                    $provider
                );
                return Ok(());
            }
        }
    };
    ($provider:expr, $capability:expr) => {
        match $crate::common::load_test_config($provider) {
            Some(cfg) if cfg.enabled => {
                if !cfg.has_capability($capability) {
                    eprintln!(
                        "Skipping test: capability '{}' not enabled for provider '{}'",
                        $capability, $provider
                    );
                    return Ok(());
                }
                match $crate::common::create_provider_and_model(&cfg) {
                    Some((provider, model)) => (provider, model),
                    None => {
                        eprintln!("Skipping test: failed to create provider '{}'", $provider);
                        return Ok(());
                    }
                }
            }
            _ => {
                eprintln!(
                    "Skipping test: provider '{}' not configured in .env",
                    $provider
                );
                return Ok(());
            }
        }
    };
}
