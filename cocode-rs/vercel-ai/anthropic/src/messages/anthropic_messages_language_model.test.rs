use std::collections::HashMap;
use std::sync::Arc;

use super::*;

fn make_config() -> Arc<AnthropicConfig> {
    Arc::new(AnthropicConfig {
        provider: "anthropic.messages".into(),
        base_url: "https://api.anthropic.com/v1".into(),
        headers: Arc::new(|| {
            let mut h = HashMap::new();
            h.insert("x-api-key".into(), "test-key".into());
            h.insert("anthropic-version".into(), "2023-06-01".into());
            h
        }),
        client: None,
    })
}

#[test]
fn creates_model_with_provider_and_id() {
    let model = AnthropicMessagesLanguageModel::new("claude-sonnet-4-5", make_config());
    assert_eq!(model.provider(), "anthropic.messages");
    assert_eq!(model.model_id(), "claude-sonnet-4-5");
}

#[test]
fn supported_urls_includes_images_and_pdfs() {
    let model = AnthropicMessagesLanguageModel::new("claude-sonnet-4-5", make_config());
    let urls = model.supported_urls();
    assert!(urls.contains_key("image/*"));
    assert!(urls.contains_key("application/pdf"));
}

#[test]
fn get_args_sets_model_and_max_tokens() {
    let model = AnthropicMessagesLanguageModel::new("claude-sonnet-4-5", make_config());
    let options = LanguageModelV4CallOptions::new(vec![
        vercel_ai_provider::LanguageModelV4Message::user_text("Hello"),
    ]);
    let (body, _headers, _warnings) = model
        .get_args(&options, false)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body["model"], "claude-sonnet-4-5");
    assert!(body["max_tokens"].is_number());
    assert!(body["system"].is_null());
    assert!(body["messages"].is_array());
}

#[test]
fn get_args_warns_on_unsupported_params() {
    let model = AnthropicMessagesLanguageModel::new("claude-sonnet-4-5", make_config());
    let mut options = LanguageModelV4CallOptions::new(vec![
        vercel_ai_provider::LanguageModelV4Message::user_text("Hello"),
    ]);
    options.frequency_penalty = Some(0.5);
    options.presence_penalty = Some(0.5);
    options.seed = Some(42);
    let (_body, _headers, warnings) = model
        .get_args(&options, false)
        .unwrap_or_else(|e| panic!("{e}"));
    let features: Vec<&str> = warnings
        .iter()
        .filter_map(|w| match w {
            Warning::Unsupported { feature, .. } => Some(feature.as_str()),
            _ => None,
        })
        .collect();
    assert!(features.contains(&"frequencyPenalty"));
    assert!(features.contains(&"presencePenalty"));
    assert!(features.contains(&"seed"));
}

#[test]
fn get_args_clamps_temperature() {
    let model = AnthropicMessagesLanguageModel::new("claude-sonnet-4-5", make_config());
    let options = LanguageModelV4CallOptions::new(vec![
        vercel_ai_provider::LanguageModelV4Message::user_text("Hello"),
    ])
    .with_temperature(2.0);
    let (body, _headers, warnings) = model
        .get_args(&options, false)
        .unwrap_or_else(|e| panic!("{e}"));
    // Temperature should be clamped to 1.0
    assert_eq!(body["temperature"], 1.0);
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, Warning::Unsupported { feature, .. } if feature == "temperature"))
    );
}

#[test]
fn get_args_includes_anthropic_beta_header() {
    let model = AnthropicMessagesLanguageModel::new("claude-sonnet-4-5", make_config());

    // Create options with tools to trigger beta header
    let tool = vercel_ai_provider::LanguageModelV4Tool::Provider(
        vercel_ai_provider::LanguageModelV4ProviderTool {
            id: "anthropic.code_execution_20250522".into(),
            name: "code_execution".into(),
            args: HashMap::new(),
        },
    );

    let options = LanguageModelV4CallOptions::new(vec![
        vercel_ai_provider::LanguageModelV4Message::user_text("Hello"),
    ])
    .with_tools(vec![tool]);

    let (_body, headers, _warnings) = model
        .get_args(&options, false)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(headers.contains_key("anthropic-beta"));
    let beta = &headers["anthropic-beta"];
    assert!(beta.contains("code-execution-2025-05-22"));
}

#[test]
fn get_args_stream_includes_stream_flag() {
    let model = AnthropicMessagesLanguageModel::new("claude-sonnet-4-5", make_config());
    let options = LanguageModelV4CallOptions::new(vec![
        vercel_ai_provider::LanguageModelV4Message::user_text("Hello"),
    ]);
    let (body, headers, _warnings) = model
        .get_args(&options, true)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body["stream"], true);
    // Should have fine-grained tool streaming beta
    assert!(
        headers
            .get("anthropic-beta")
            .map(|b| b.contains("fine-grained-tool-streaming"))
            .unwrap_or(false)
    );
}
