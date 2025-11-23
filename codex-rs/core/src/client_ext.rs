//! Extension module for ModelClient adapter integration.
//!
//! This module provides adapter support with minimal changes to the main client.rs file.
//! Following the upstream sync pattern to minimize merge conflicts.

use crate::adapters::RequestContext;
use crate::adapters::http::AdapterHttpClient;
use crate::client::ModelClient;
use crate::client_common::Prompt;
use crate::client_common::ResponseStream;
use crate::config::Config;
use crate::error::CodexErr;
use crate::error::Result;
use crate::model_provider_info::ModelProviderInfo;
use codex_protocol::config_types_ext::ModelParameters;
use codex_protocol::protocol::SessionSource;

/// Resolve effective model parameters by merging config and provider settings.
///
/// Priority (highest to lowest):
/// 1. Provider-specific parameters (`provider.ext.model_parameters`)
/// 2. Global config parameters (`config.ext.model_parameters`)
/// 3. Defaults (empty/None values)
///
/// Note: model_max_output_tokens is NOT applied here. It should be set in
/// config.ext.model_parameters.max_tokens if needed, or via provider override.
pub fn resolve_parameters(config: &Config, provider: &ModelProviderInfo) -> ModelParameters {
    // Start with global config parameters (or defaults)
    let mut params = config.ext.model_parameters.clone().unwrap_or_default();

    // Apply provider-level overrides (non-None values take precedence)
    if let Some(provider_params) = &provider.ext.model_parameters {
        if provider_params.temperature.is_some() {
            params.temperature = provider_params.temperature;
        }
        if provider_params.top_p.is_some() {
            params.top_p = provider_params.top_p;
        }
        if provider_params.frequency_penalty.is_some() {
            params.frequency_penalty = provider_params.frequency_penalty;
        }
        if provider_params.presence_penalty.is_some() {
            params.presence_penalty = provider_params.presence_penalty;
        }
        if provider_params.max_tokens.is_some() {
            params.max_tokens = provider_params.max_tokens;
        }
    }

    // Apply model_max_output_tokens if set (highest priority for max_tokens)
    if let Some(max_output) = config.ext.model_max_output_tokens {
        params.max_tokens = Some(max_output);
    }

    params
}

/// Convert SessionSource to string representation for RequestContext.
///
/// Maps SessionSource enum variants to their string equivalents for use in
/// adapter request context.
fn session_source_to_string(source: &SessionSource) -> String {
    match source {
        SessionSource::Cli => "Cli".to_string(),
        SessionSource::VSCode => "VSCode".to_string(),
        SessionSource::Exec => "Exec".to_string(),
        SessionSource::Mcp => "Mcp".to_string(),
        SessionSource::SubAgent(_) => "SubAgent".to_string(),
        SessionSource::Unknown => "Unknown".to_string(),
    }
}

/// Stream responses using provider adapter.
///
/// This function is called from `ModelClient::stream()` when `provider.ext.adapter` is set.
/// It builds the RequestContext with effective parameters and delegates to the adapter HTTP layer.
///
/// # Arguments
///
/// * `client` - The ModelClient instance (provides access to config, auth, otel, etc.)
/// * `prompt` - The conversation prompt to send to the model
///
/// # Returns
///
/// A `ResponseStream` from the adapter, or an error if adapter setup fails.
pub async fn stream_with_adapter(client: &ModelClient, prompt: &Prompt) -> Result<ResponseStream> {
    let provider = client.get_provider();

    // Verify adapter is configured
    let adapter_name = provider.ext.adapter.as_ref().ok_or_else(|| {
        CodexErr::Fatal("stream_with_adapter called but adapter not configured".into())
    })?;

    // Calculate effective verbosity (same logic as client.rs for Responses API)
    let verbosity = if client.config().model_family.support_verbosity {
        client
            .config()
            .model_verbosity
            .or(client.config().model_family.default_verbosity)
    } else {
        None
    };

    // Build RequestContext with effective parameters
    let context = RequestContext {
        conversation_id: client.get_conversation_id().to_string(),
        session_source: session_source_to_string(&client.get_session_source()),
        effective_parameters: resolve_parameters(&client.config(), &provider),
        reasoning_effort: client.get_reasoning_effort(),
        reasoning_summary: Some(client.get_reasoning_summary()),
        verbosity,
    };

    // Create adapter HTTP client
    let adapter_client =
        AdapterHttpClient::new(client.get_http_client(), client.get_otel_event_manager());

    // Delegate to adapter HTTP layer
    adapter_client
        .stream_with_adapter(
            prompt,
            context,
            &provider,
            adapter_name,
            provider.stream_idle_timeout_ms,
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::config::ConfigToml;
    use crate::model_provider_info::ModelProviderInfo;
    use codex_protocol::config_types_ext::ModelParameters;
    use std::path::PathBuf;

    // Test helper to create minimal Config
    fn create_test_config() -> Config {
        let cfg = ConfigToml::default();
        Config::load_from_base_config_with_overrides(
            cfg,
            Default::default(),
            PathBuf::from("/tmp/test_codex_home"),
        )
        .expect("failed to create test config")
    }

    #[test]
    fn test_resolve_parameters_with_defaults() {
        let config = create_test_config();
        let provider = ModelProviderInfo::default();

        let params = resolve_parameters(&config, &provider);

        // All should be None/default
        assert_eq!(params.temperature, None);
        assert_eq!(params.top_p, None);
        assert_eq!(params.max_tokens, None);
    }

    #[test]
    fn test_resolve_parameters_provider_overrides_config() {
        let mut config = create_test_config();
        config.ext.model_parameters = Some(ModelParameters {
            temperature: Some(0.5),
            top_p: Some(0.9),
            max_tokens: Some(1000),
            ..Default::default()
        });

        let mut provider = ModelProviderInfo::default();
        provider.ext.model_parameters = Some(ModelParameters {
            temperature: Some(0.7), // Override
            // top_p not set - should keep config value
            max_tokens: Some(2000), // Override
            ..Default::default()
        });

        let params = resolve_parameters(&config, &provider);

        assert_eq!(params.temperature, Some(0.7)); // Provider wins
        assert_eq!(params.top_p, Some(0.9)); // Config value kept
        assert_eq!(params.max_tokens, Some(2000)); // Provider wins
    }

    #[test]
    fn test_resolve_parameters_model_max_output_tokens_highest_priority() {
        let mut config = create_test_config();
        config.ext.model_max_output_tokens = Some(10000);
        config.ext.model_parameters = Some(ModelParameters {
            max_tokens: Some(1000),
            ..Default::default()
        });

        let mut provider = ModelProviderInfo::default();
        provider.ext.model_parameters = Some(ModelParameters {
            max_tokens: Some(2000),
            ..Default::default()
        });

        let params = resolve_parameters(&config, &provider);

        // model_max_output_tokens should win over both
        assert_eq!(params.max_tokens, Some(10000));
    }

    #[test]
    fn test_session_source_to_string_all_variants() {
        assert_eq!(session_source_to_string(&SessionSource::Cli), "Cli");
        assert_eq!(session_source_to_string(&SessionSource::VSCode), "VSCode");
        assert_eq!(session_source_to_string(&SessionSource::Exec), "Exec");
        assert_eq!(session_source_to_string(&SessionSource::Mcp), "Mcp");
        assert_eq!(session_source_to_string(&SessionSource::Unknown), "Unknown");

        use codex_protocol::protocol::SubAgentSource;
        assert_eq!(
            session_source_to_string(&SessionSource::SubAgent(SubAgentSource::Review)),
            "SubAgent"
        );
    }
}
