//! Live integration tests for cocode-api.
//!
//! These tests exercise the ApiClient, UnifiedStream, and provider_factory
//! layers against real LLM providers. Configure via `.env.test` or `.env`.
//!
//! Run with: `cargo test -p cocode-api --test live -- --test-threads=1`

mod common;

mod live {
    pub mod suite;
}

use anyhow::Result;

// ---------------------------------------------------------------------------
// Test macros
// ---------------------------------------------------------------------------

/// Run a test suite function for a specific provider with capability gating.
macro_rules! provider_test {
    ($provider:expr, $test_fn:path) => {{
        let (client, model, _cfg) = require_api_provider!($provider);
        $test_fn(&client, &*model).await
    }};
    ($provider:expr, $capability:expr, $test_fn:path) => {{
        let (client, model, _cfg) = require_api_provider!($provider, $capability);
        $test_fn(&client, &*model).await
    }};
}

/// Run a provider_factory test (takes ProviderInfo instead of ApiClient).
macro_rules! factory_test {
    ($provider:expr, $test_fn:path) => {{
        let cfg = match common::load_provider_config($provider) {
            Some(cfg) if cfg.enabled => cfg,
            _ => {
                eprintln!(
                    "Skipping test: provider '{}' not configured in .env",
                    $provider
                );
                return Ok(());
            }
        };
        $test_fn(&cfg.provider_info).await
    }};
    ($provider:expr, $test_fn:path, model) => {{
        let cfg = match common::load_provider_config($provider) {
            Some(cfg) if cfg.enabled => cfg,
            _ => {
                eprintln!(
                    "Skipping test: provider '{}' not configured in .env",
                    $provider
                );
                return Ok(());
            }
        };
        $test_fn(&cfg.provider_info, &cfg.model_slug).await
    }};
}

// ===========================================================================
// Basic text generation tests
// ===========================================================================

#[tokio::test]
async fn test_basic_openai() -> Result<()> {
    provider_test!("openai", "text", live::suite::basic::run_text)
}

#[tokio::test]
async fn test_basic_openai_chat() -> Result<()> {
    provider_test!("openai_chat", "text", live::suite::basic::run_text)
}

#[tokio::test]
async fn test_basic_anthropic() -> Result<()> {
    provider_test!("anthropic", "text", live::suite::basic::run_text)
}

#[tokio::test]
async fn test_basic_gemini() -> Result<()> {
    provider_test!("gemini", "text", live::suite::basic::run_text)
}

#[tokio::test]
async fn test_basic_volcengine() -> Result<()> {
    provider_test!("volcengine", "text", live::suite::basic::run_text)
}

#[tokio::test]
async fn test_basic_zai() -> Result<()> {
    provider_test!("zai", "text", live::suite::basic::run_text)
}

#[tokio::test]
async fn test_basic_openai_compat() -> Result<()> {
    provider_test!("openai_compat", "text", live::suite::basic::run_text)
}

// ===========================================================================
// Non-streaming tests
// ===========================================================================

#[tokio::test]
async fn test_non_streaming_openai() -> Result<()> {
    provider_test!("openai", "text", live::suite::basic::run_non_streaming)
}

#[tokio::test]
async fn test_non_streaming_openai_chat() -> Result<()> {
    provider_test!("openai_chat", "text", live::suite::basic::run_non_streaming)
}

#[tokio::test]
async fn test_non_streaming_anthropic() -> Result<()> {
    provider_test!("anthropic", "text", live::suite::basic::run_non_streaming)
}

#[tokio::test]
async fn test_non_streaming_gemini() -> Result<()> {
    provider_test!("gemini", "text", live::suite::basic::run_non_streaming)
}

#[tokio::test]
async fn test_non_streaming_openai_compat() -> Result<()> {
    provider_test!(
        "openai_compat",
        "text",
        live::suite::basic::run_non_streaming
    )
}

// ===========================================================================
// Token usage tests
// ===========================================================================

#[tokio::test]
async fn test_token_usage_openai() -> Result<()> {
    provider_test!("openai", "text", live::suite::basic::run_token_usage)
}

#[tokio::test]
async fn test_token_usage_anthropic() -> Result<()> {
    provider_test!("anthropic", "text", live::suite::basic::run_token_usage)
}

#[tokio::test]
async fn test_token_usage_gemini() -> Result<()> {
    provider_test!("gemini", "text", live::suite::basic::run_token_usage)
}

// ===========================================================================
// Multi-turn tests
// ===========================================================================

#[tokio::test]
async fn test_multi_turn_openai() -> Result<()> {
    provider_test!("openai", "text", live::suite::basic::run_multi_turn)
}

#[tokio::test]
async fn test_multi_turn_anthropic() -> Result<()> {
    provider_test!("anthropic", "text", live::suite::basic::run_multi_turn)
}

#[tokio::test]
async fn test_multi_turn_gemini() -> Result<()> {
    provider_test!("gemini", "text", live::suite::basic::run_multi_turn)
}

// ===========================================================================
// Streaming tests
// ===========================================================================

#[tokio::test]
async fn test_streaming_openai() -> Result<()> {
    provider_test!(
        "openai",
        "streaming",
        live::suite::streaming::run_stream_events
    )
}

#[tokio::test]
async fn test_streaming_openai_chat() -> Result<()> {
    provider_test!(
        "openai_chat",
        "streaming",
        live::suite::streaming::run_stream_events
    )
}

#[tokio::test]
async fn test_streaming_anthropic() -> Result<()> {
    provider_test!(
        "anthropic",
        "streaming",
        live::suite::streaming::run_stream_events
    )
}

#[tokio::test]
async fn test_streaming_gemini() -> Result<()> {
    provider_test!(
        "gemini",
        "streaming",
        live::suite::streaming::run_stream_events
    )
}

#[tokio::test]
async fn test_streaming_content_openai() -> Result<()> {
    provider_test!(
        "openai",
        "streaming",
        live::suite::streaming::run_stream_content
    )
}

#[tokio::test]
async fn test_streaming_content_anthropic() -> Result<()> {
    provider_test!(
        "anthropic",
        "streaming",
        live::suite::streaming::run_stream_content
    )
}

#[tokio::test]
async fn test_streaming_event_tx_openai() -> Result<()> {
    provider_test!(
        "openai",
        "streaming",
        live::suite::streaming::run_stream_event_tx
    )
}

// ===========================================================================
// Tool calling tests
// ===========================================================================

#[tokio::test]
async fn test_tools_openai() -> Result<()> {
    provider_test!("openai", "tools", live::suite::tools::run_tool_call)
}

#[tokio::test]
async fn test_tools_openai_chat() -> Result<()> {
    provider_test!("openai_chat", "tools", live::suite::tools::run_tool_call)
}

#[tokio::test]
async fn test_tools_anthropic() -> Result<()> {
    provider_test!("anthropic", "tools", live::suite::tools::run_tool_call)
}

#[tokio::test]
async fn test_tools_gemini() -> Result<()> {
    provider_test!("gemini", "tools", live::suite::tools::run_tool_call)
}

#[tokio::test]
async fn test_tools_streaming_openai() -> Result<()> {
    provider_test!(
        "openai",
        "tools",
        live::suite::tools::run_tool_call_streaming
    )
}

#[tokio::test]
async fn test_tools_streaming_anthropic() -> Result<()> {
    provider_test!(
        "anthropic",
        "tools",
        live::suite::tools::run_tool_call_streaming
    )
}

// ===========================================================================
// Provider factory tests
// ===========================================================================

#[tokio::test]
async fn test_factory_provider_openai() -> Result<()> {
    factory_test!("openai", live::suite::provider_factory::run_create_provider)
}

#[tokio::test]
async fn test_factory_provider_anthropic() -> Result<()> {
    factory_test!(
        "anthropic",
        live::suite::provider_factory::run_create_provider
    )
}

#[tokio::test]
async fn test_factory_model_openai() -> Result<()> {
    factory_test!(
        "openai",
        live::suite::provider_factory::run_create_model,
        model
    )
}

#[tokio::test]
async fn test_factory_model_anthropic() -> Result<()> {
    factory_test!(
        "anthropic",
        live::suite::provider_factory::run_create_model,
        model
    )
}

#[tokio::test]
async fn test_factory_model_gemini() -> Result<()> {
    factory_test!(
        "gemini",
        live::suite::provider_factory::run_create_model,
        model
    )
}

// ===========================================================================
// Cross-provider tests (dynamic all-pairs)
// ===========================================================================

#[tokio::test]
async fn test_cross_provider_all() -> Result<()> {
    let providers = common::config::list_cross_provider_configs();
    if providers.len() < 2 {
        eprintln!(
            "Skipping cross-provider tests: need >= 2 providers with cross_provider capability, got {}",
            providers.len()
        );
        return Ok(());
    }

    let mut failures = vec![];

    for (i, provider_a) in providers.iter().enumerate() {
        for (j, provider_b) in providers.iter().enumerate() {
            if i == j {
                continue;
            }
            eprintln!(
                "Cross-provider: {} -> {}",
                provider_a.provider, provider_b.provider
            );

            let model_a =
                match cocode_api::create_model(&provider_a.provider_info, &provider_a.model_slug) {
                    Ok(m) => m,
                    Err(e) => {
                        failures.push(format!("{}: create_model failed: {e}", provider_a.provider));
                        continue;
                    }
                };
            let model_b =
                match cocode_api::create_model(&provider_b.provider_info, &provider_b.model_slug) {
                    Ok(m) => m,
                    Err(e) => {
                        failures.push(format!("{}: create_model failed: {e}", provider_b.provider));
                        continue;
                    }
                };

            let client = cocode_api::ApiClient::new();

            // A generates, B continues with A's response as history
            let request_a = cocode_api::LanguageModelCallOptions::new(vec![
                cocode_api::LanguageModelMessage::user_text(
                    "What is 2+2? Reply with just the number.",
                ),
            ]);

            let response_a = match client
                .stream_request(&*model_a, request_a, cocode_api::StreamOptions::streaming())
                .await
            {
                Ok(stream) => match stream.collect().await {
                    Ok(r) => r,
                    Err(e) => {
                        failures.push(format!(
                            "{} -> {}: collect A failed: {e}",
                            provider_a.provider, provider_b.provider
                        ));
                        continue;
                    }
                },
                Err(e) => {
                    failures.push(format!(
                        "{} -> {}: stream A failed: {e}",
                        provider_a.provider, provider_b.provider
                    ));
                    continue;
                }
            };

            let text_a = response_a.text();
            let request_b = cocode_api::LanguageModelCallOptions::new(vec![
                cocode_api::LanguageModelMessage::user_text(
                    "What is 2+2? Reply with just the number.",
                ),
                cocode_api::LanguageModelMessage::assistant_text(&text_a),
                cocode_api::LanguageModelMessage::user_text(
                    "What is double that number? Reply with just the number.",
                ),
            ]);

            match client
                .stream_request(&*model_b, request_b, cocode_api::StreamOptions::streaming())
                .await
            {
                Ok(stream) => match stream.collect().await {
                    Ok(r) => {
                        let text = r.text();
                        if !text.contains('8') {
                            failures.push(format!(
                                "{} -> {}: expected '8', got: {}",
                                provider_a.provider,
                                provider_b.provider,
                                text.trim()
                            ));
                        }
                    }
                    Err(e) => {
                        failures.push(format!(
                            "{} -> {}: collect B failed: {e}",
                            provider_a.provider, provider_b.provider
                        ));
                    }
                },
                Err(e) => {
                    failures.push(format!(
                        "{} -> {}: stream B failed: {e}",
                        provider_a.provider, provider_b.provider
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "Cross-provider failures:\n{}",
        failures.join("\n")
    );

    Ok(())
}
