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
        supports_native_structured_output: None,
        supports_strict_tools: None,
        full_url: None,
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

// ---------------------------------------------------------------------------
// Skills validation: code execution tool required
// ---------------------------------------------------------------------------

#[test]
fn skills_without_code_execution_tool_produces_warning() {
    let model = AnthropicMessagesLanguageModel::new("claude-sonnet-4-5", make_config());

    // Container with skills but no code execution tool
    let mut anthropic_opts: HashMap<String, serde_json::Value> = HashMap::new();
    anthropic_opts.insert(
        "container".into(),
        json!({
            "skills": [{"type": "skill", "skillId": "my_skill"}]
        }),
    );
    let mut provider_opts = HashMap::new();
    provider_opts.insert("anthropic".into(), anthropic_opts);

    let options = LanguageModelV4CallOptions::new(vec![
        vercel_ai_provider::LanguageModelV4Message::user_text("Hello"),
    ])
    .with_provider_options(vercel_ai_provider::ProviderOptions(provider_opts));

    let (_body, _headers, warnings) = model
        .get_args(&options, false)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            Warning::Other { message }
                if message.contains("code execution tool is required when using skills")
        )),
        "expected skills validation warning, got: {warnings:?}",
    );
}

#[test]
fn skills_with_code_execution_tool_no_warning() {
    let model = AnthropicMessagesLanguageModel::new("claude-sonnet-4-5", make_config());

    let mut anthropic_opts: HashMap<String, serde_json::Value> = HashMap::new();
    anthropic_opts.insert(
        "container".into(),
        json!({
            "skills": [{"type": "skill", "skillId": "my_skill"}]
        }),
    );
    let mut provider_opts = HashMap::new();
    provider_opts.insert("anthropic".into(), anthropic_opts);

    let ce_tool = vercel_ai_provider::LanguageModelV4Tool::Provider(
        vercel_ai_provider::LanguageModelV4ProviderTool {
            id: "anthropic.code_execution_20250825".into(),
            name: "code_execution".into(),
            args: HashMap::new(),
        },
    );

    let options = LanguageModelV4CallOptions::new(vec![
        vercel_ai_provider::LanguageModelV4Message::user_text("Hello"),
    ])
    .with_tools(vec![ce_tool])
    .with_provider_options(vercel_ai_provider::ProviderOptions(provider_opts));

    let (_body, _headers, warnings) = model
        .get_args(&options, false)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        !warnings.iter().any(|w| matches!(
            w,
            Warning::Other { message }
                if message.contains("code execution tool is required")
        )),
        "should not warn when code_execution tool is present: {warnings:?}",
    );
}

// ---------------------------------------------------------------------------
// is_known_model + max_tokens clamping
// ---------------------------------------------------------------------------

#[test]
fn known_model_clamps_max_tokens_with_warning() {
    let model = AnthropicMessagesLanguageModel::new("claude-sonnet-4-5", make_config());
    // claude-sonnet-4-5 has max_output_tokens = 64_000
    let options = LanguageModelV4CallOptions::new(vec![
        vercel_ai_provider::LanguageModelV4Message::user_text("Hello"),
    ])
    .with_max_output_tokens(100_000);
    let (body, _headers, warnings) = model
        .get_args(&options, false)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body["max_tokens"], 64_000);
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            Warning::Unsupported { feature, .. } if feature == "maxOutputTokens"
        )),
        "expected maxOutputTokens warning, got: {warnings:?}",
    );
}

#[test]
fn known_model_clamps_without_warning_when_no_explicit_max() {
    // Unknown model should not clamp
    let model = AnthropicMessagesLanguageModel::new("some-custom-model", make_config());
    let options = LanguageModelV4CallOptions::new(vec![
        vercel_ai_provider::LanguageModelV4Message::user_text("Hello"),
    ]);
    let (body, _headers, warnings) = model
        .get_args(&options, false)
        .unwrap_or_else(|e| panic!("{e}"));
    // Unknown model defaults to 4096
    assert_eq!(body["max_tokens"], 4096);
    assert!(!warnings.iter().any(
        |w| matches!(w, Warning::Unsupported { feature, .. } if feature == "maxOutputTokens")
    ),);
}

// ---------------------------------------------------------------------------
// JSON response format warning when schema is null
// ---------------------------------------------------------------------------

#[test]
fn json_response_format_without_schema_warns() {
    let model = AnthropicMessagesLanguageModel::new("claude-sonnet-4-5", make_config());
    let options = LanguageModelV4CallOptions::new(vec![
        vercel_ai_provider::LanguageModelV4Message::user_text("Hello"),
    ])
    .with_response_format(vercel_ai_provider::ResponseFormat::Json {
        schema: None,
        name: None,
        description: None,
    });
    let (_body, _headers, warnings) = model
        .get_args(&options, false)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            Warning::Unsupported { feature, details }
                if feature == "responseFormat"
                && details.as_deref().unwrap_or("").contains("requires a schema")
        )),
        "expected responseFormat warning, got: {warnings:?}",
    );
}

// ---------------------------------------------------------------------------
// Thinking budget warning uses Compatibility
// ---------------------------------------------------------------------------

#[test]
fn thinking_budget_default_uses_compatibility_warning() {
    let model = AnthropicMessagesLanguageModel::new("claude-sonnet-4-5", make_config());

    let mut anthropic_opts: HashMap<String, serde_json::Value> = HashMap::new();
    anthropic_opts.insert(
        "thinking".into(),
        json!({"type": "enabled"}), // no budget_tokens
    );
    let mut provider_opts = HashMap::new();
    provider_opts.insert("anthropic".into(), anthropic_opts);

    let options = LanguageModelV4CallOptions::new(vec![
        vercel_ai_provider::LanguageModelV4Message::user_text("Hello"),
    ])
    .with_provider_options(vercel_ai_provider::ProviderOptions(provider_opts));

    let (_body, _headers, warnings) = model
        .get_args(&options, false)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            Warning::Compatibility { feature, .. } if feature == "extended thinking"
        )),
        "expected Compatibility warning for thinking budget, got: {warnings:?}",
    );
    // Should NOT have Warning::Other for this
    assert!(!warnings.iter().any(|w| matches!(
        w,
        Warning::Other { message } if message.contains("thinking budget")
    )),);
}

// ---------------------------------------------------------------------------
// context_management camelCase → snake_case transform
// ---------------------------------------------------------------------------

#[test]
fn context_management_transforms_camel_to_snake() {
    let model = AnthropicMessagesLanguageModel::new("claude-sonnet-4-5", make_config());

    let mut anthropic_opts: HashMap<String, serde_json::Value> = HashMap::new();
    anthropic_opts.insert(
        "contextManagement".into(),
        json!({
            "edits": [
                {
                    "type": "clear_tool_uses_20250919",
                    "trigger": "auto",
                    "keep": 5,
                    "clearAtLeast": 3,
                    "clearToolInputs": true,
                    "excludeTools": ["tool_a"]
                },
                {
                    "type": "clear_thinking_20251015",
                    "keep": 2
                },
                {
                    "type": "compact_20260112",
                    "trigger": "auto",
                    "pauseAfterCompaction": true,
                    "instructions": "summarize"
                }
            ]
        }),
    );
    let mut provider_opts = HashMap::new();
    provider_opts.insert("anthropic".into(), anthropic_opts);

    let options = LanguageModelV4CallOptions::new(vec![
        vercel_ai_provider::LanguageModelV4Message::user_text("Hello"),
    ])
    .with_provider_options(vercel_ai_provider::ProviderOptions(provider_opts));

    let (body, _headers, _warnings) = model
        .get_args(&options, false)
        .unwrap_or_else(|e| panic!("{e}"));

    let ctx = &body["context_management"];
    let edits = ctx["edits"].as_array().expect("edits should be array");
    assert_eq!(edits.len(), 3);

    // clear_tool_uses_20250919: camelCase → snake_case
    let edit0 = &edits[0];
    assert_eq!(edit0["type"], "clear_tool_uses_20250919");
    assert_eq!(edit0["clear_at_least"], 3);
    assert_eq!(edit0["clear_tool_inputs"], true);
    assert_eq!(edit0["exclude_tools"], json!(["tool_a"]));
    // Should NOT have camelCase keys
    assert!(edit0.get("clearAtLeast").is_none());
    assert!(edit0.get("clearToolInputs").is_none());
    assert!(edit0.get("excludeTools").is_none());

    // compact_20260112: pauseAfterCompaction → pause_after_compaction
    let edit2 = &edits[2];
    assert_eq!(edit2["type"], "compact_20260112");
    assert_eq!(edit2["pause_after_compaction"], true);
    assert!(edit2.get("pauseAfterCompaction").is_none());
}

#[test]
fn context_management_unknown_strategy_warns() {
    let model = AnthropicMessagesLanguageModel::new("claude-sonnet-4-5", make_config());

    let mut anthropic_opts: HashMap<String, serde_json::Value> = HashMap::new();
    anthropic_opts.insert(
        "contextManagement".into(),
        json!({
            "edits": [
                {"type": "unknown_strategy_20991231"}
            ]
        }),
    );
    let mut provider_opts = HashMap::new();
    provider_opts.insert("anthropic".into(), anthropic_opts);

    let options = LanguageModelV4CallOptions::new(vec![
        vercel_ai_provider::LanguageModelV4Message::user_text("Hello"),
    ])
    .with_provider_options(vercel_ai_provider::ProviderOptions(provider_opts));

    let (body, _headers, warnings) = model
        .get_args(&options, false)
        .unwrap_or_else(|e| panic!("{e}"));

    // Unknown strategy should be filtered out
    let edits = body["context_management"]["edits"]
        .as_array()
        .expect("edits should be array");
    assert_eq!(edits.len(), 0);

    // Should have warning
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            Warning::Other { message } if message.contains("Unknown context management strategy")
        )),
        "expected unknown strategy warning, got: {warnings:?}",
    );
}
