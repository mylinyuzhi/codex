//! Provider implementations.

pub mod anthropic;
pub mod gemini;
pub mod openai;
pub mod openai_compat;
pub mod volcengine;
pub mod zai;

pub use anthropic::AnthropicProvider;
pub use gemini::GeminiProvider;
pub use openai::OpenAIProvider;
pub use openai_compat::OpenAICompatProvider;
pub use volcengine::VolcengineProvider;
pub use zai::ZaiProvider;

use crate::error::HyperError;
use crate::registry::register_provider;
use std::sync::Arc;

/// Initialize all built-in providers from environment variables.
///
/// This attempts to create providers for all known services using
/// their standard environment variables (OPENAI_API_KEY, ANTHROPIC_API_KEY, etc.).
///
/// Providers that fail to initialize (e.g., missing API keys) are silently skipped.
pub fn init_from_env() {
    // OpenAI
    if let Ok(provider) = OpenAIProvider::from_env() {
        register_provider(Arc::new(provider));
    }

    // Anthropic
    if let Ok(provider) = AnthropicProvider::from_env() {
        register_provider(Arc::new(provider));
    }

    // Gemini
    if let Ok(provider) = GeminiProvider::from_env() {
        register_provider(Arc::new(provider));
    }

    // Volcengine Ark
    if let Ok(provider) = VolcengineProvider::from_env() {
        register_provider(Arc::new(provider));
    }

    // Z.AI / ZhipuAI
    if let Ok(provider) = ZaiProvider::from_env() {
        register_provider(Arc::new(provider));
    }
}

/// Try to create a provider from environment variables.
///
/// Returns the first provider that can be created, or an error if none can be created.
pub fn any_from_env() -> Result<Arc<dyn crate::provider::Provider>, HyperError> {
    // Try providers in order of preference
    if let Ok(provider) = OpenAIProvider::from_env() {
        return Ok(Arc::new(provider));
    }

    if let Ok(provider) = AnthropicProvider::from_env() {
        return Ok(Arc::new(provider));
    }

    if let Ok(provider) = GeminiProvider::from_env() {
        return Ok(Arc::new(provider));
    }

    if let Ok(provider) = VolcengineProvider::from_env() {
        return Ok(Arc::new(provider));
    }

    if let Ok(provider) = ZaiProvider::from_env() {
        return Ok(Arc::new(provider));
    }

    Err(HyperError::ConfigError(
        "No provider could be initialized. Set OPENAI_API_KEY, ANTHROPIC_API_KEY, GOOGLE_API_KEY, ARK_API_KEY, or ZAI_API_KEY."
            .to_string(),
    ))
}
