use super::*;
use crate::LanguageModelFunctionTool;
use cocode_protocol::execution::AgentKind;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelInfo;
use cocode_protocol::model::ModelSpec;
use cocode_protocol::thinking::ThinkingLevel;

fn sample_context() -> InferenceContext {
    let spec = ModelSpec::new("anthropic", "claude-opus-4");
    let info = ModelInfo {
        slug: "claude-opus-4".to_string(),
        context_window: Some(200000),
        max_output_tokens: Some(16384),
        temperature: Some(1.0),
        top_p: Some(0.9),
        ..Default::default()
    };

    InferenceContext::new(
        "call-123",
        "session-456",
        1,
        spec,
        info,
        AgentKind::Main,
        ExecutionIdentity::main(),
    )
}

#[test]
fn test_basic_build() {
    let ctx = sample_context();
    let messages = vec![LanguageModelMessage::user_text("Hello")];

    let request = RequestBuilder::new(ctx).messages(messages).build();

    assert_eq!(request.prompt.len(), 1);
    assert_eq!(request.temperature, Some(1.0));
    // top_p comparison with tolerance due to f32->f64 conversion
    assert!((request.top_p.unwrap() - 0.9).abs() < 0.001);
    assert_eq!(request.max_output_tokens, Some(16384));
}

#[test]
fn test_override_temperature() {
    let ctx = sample_context();
    let messages = vec![LanguageModelMessage::user_text("Hello")];

    let request = RequestBuilder::new(ctx)
        .messages(messages)
        .temperature(0.5)
        .build();

    assert_eq!(request.temperature, Some(0.5));
}

#[test]
fn test_override_max_tokens() {
    let ctx = sample_context();
    let messages = vec![LanguageModelMessage::user_text("Hello")];

    let request = RequestBuilder::new(ctx)
        .messages(messages)
        .max_tokens(1000)
        .build();

    assert_eq!(request.max_output_tokens, Some(1000));
}

#[test]
fn test_with_tools() {
    let ctx = sample_context();
    let messages = vec![LanguageModelMessage::user_text("Hello")];
    let tools = vec![LanguageModelTool::function(
        LanguageModelFunctionTool::with_description(
            "test_tool",
            "A test tool",
            serde_json::json!({"type": "object"}),
        ),
    )];

    let request = RequestBuilder::new(ctx)
        .messages(messages)
        .tools(tools)
        .build();

    assert!(request.tools.is_some());
    assert_eq!(request.tools.as_ref().unwrap().len(), 1);
}

#[test]
fn test_with_thinking() {
    let spec = ModelSpec::new("openai", "o1");
    let info = ModelInfo {
        slug: "o1".to_string(),
        context_window: Some(128000),
        max_output_tokens: Some(32768),
        default_thinking_level: Some(ThinkingLevel::high()),
        ..Default::default()
    };

    let ctx = InferenceContext::new(
        "call-123",
        "session-456",
        1,
        spec,
        info,
        AgentKind::Main,
        ExecutionIdentity::main(),
    );

    let messages = vec![LanguageModelMessage::user_text("Hello")];
    let request = RequestBuilder::new(ctx).messages(messages).build();

    // Should have provider options for thinking
    assert!(request.provider_options.is_some());
}

#[test]
fn test_context_accessor() {
    let ctx = sample_context();
    let builder = RequestBuilder::new(ctx);

    assert_eq!(builder.context().session_id, "session-456");
    assert_eq!(builder.context().turn_number, 1);
}

#[test]
fn test_build_request_helper() {
    let ctx = sample_context();
    let messages = vec![LanguageModelMessage::user_text("Hello")];

    let request = build_request(ctx, messages, None);

    assert_eq!(request.prompt.len(), 1);
    assert!(request.tools.is_none());
}

#[test]
fn test_interceptors_applied_as_headers() {
    let ctx = sample_context().with_interceptor_names(vec!["byted_model_hub".to_string()]);
    let messages = vec![LanguageModelMessage::user_text("Hello")];

    let request = RequestBuilder::new(ctx).messages(messages).build();

    // The byted_model_hub interceptor adds an "extra" header with session_id
    assert!(request.headers.is_some());
    let headers = request.headers.unwrap();
    assert!(
        headers.contains_key("extra"),
        "byted_model_hub interceptor should add 'extra' header"
    );
    // The header value should contain session_id JSON
    let extra = &headers["extra"];
    assert!(extra.contains("session-456"), "should contain session_id");
}

#[test]
fn test_no_interceptors_no_headers() {
    let ctx = sample_context(); // no interceptor_names
    let messages = vec![LanguageModelMessage::user_text("Hello")];

    let request = RequestBuilder::new(ctx).messages(messages).build();

    // No interceptors configured, so no extra headers
    assert!(request.headers.is_none());
}

// =========================================================================
// P16: Provider base options injection
// =========================================================================

#[test]
fn test_openai_base_options_store_false() {
    let spec = ModelSpec::new("openai", "gpt-4o");
    let info = ModelInfo {
        slug: "gpt-4o".to_string(),
        context_window: Some(128000),
        max_output_tokens: Some(16384),
        ..Default::default()
    };

    let ctx = InferenceContext::new(
        "call-1",
        "session-1",
        1,
        spec,
        info,
        AgentKind::Main,
        ExecutionIdentity::main(),
    );

    let messages = vec![LanguageModelMessage::user_text("Hello")];
    let request = RequestBuilder::new(ctx).messages(messages).build();

    let opts = request
        .provider_options
        .expect("should have provider options");
    let openai = opts.0.get("openai").expect("should have openai entry");
    assert_eq!(
        openai.get("store"),
        Some(&serde_json::json!(false)),
        "OpenAI requests should default store=false"
    );
}

#[test]
fn test_gemini_base_options_thinking_config() {
    let spec = ModelSpec::new("gemini", "gemini-2.5-pro");
    let info = ModelInfo {
        slug: "gemini-2.5-pro".to_string(),
        context_window: Some(1000000),
        max_output_tokens: Some(65536),
        ..Default::default()
    };

    let ctx = InferenceContext::new(
        "call-1",
        "session-1",
        1,
        spec,
        info,
        AgentKind::Main,
        ExecutionIdentity::main(),
    );

    let messages = vec![LanguageModelMessage::user_text("Hello")];
    let request = RequestBuilder::new(ctx).messages(messages).build();

    let opts = request
        .provider_options
        .expect("should have provider options");
    let google = opts.0.get("google").expect("should have google entry");
    assert_eq!(
        google.get("thinkingConfig"),
        Some(&serde_json::json!({"includeThoughts": true})),
        "Gemini requests should default includeThoughts=true"
    );
}

#[test]
fn test_base_options_overridden_by_request_options() {
    let spec = ModelSpec::new("openai", "gpt-4o");
    let info = ModelInfo {
        slug: "gpt-4o".to_string(),
        context_window: Some(128000),
        max_output_tokens: Some(16384),
        ..Default::default()
    };

    let mut request_opts = std::collections::HashMap::new();
    request_opts.insert("store".to_string(), serde_json::json!(true));

    let ctx = InferenceContext::new(
        "call-1",
        "session-1",
        1,
        spec,
        info,
        AgentKind::Main,
        ExecutionIdentity::main(),
    )
    .with_request_options(request_opts);

    let messages = vec![LanguageModelMessage::user_text("Hello")];
    let request = RequestBuilder::new(ctx).messages(messages).build();

    let opts = request
        .provider_options
        .expect("should have provider options");
    let openai = opts.0.get("openai").expect("should have openai entry");
    assert_eq!(
        openai.get("store"),
        Some(&serde_json::json!(true)),
        "User request_options should override base options"
    );
}

#[test]
fn test_anthropic_no_base_options() {
    let ctx = sample_context(); // Anthropic provider
    let messages = vec![LanguageModelMessage::user_text("Hello")];

    let request = RequestBuilder::new(ctx).messages(messages).build();

    // Anthropic has no base provider options (unless thinking is enabled)
    assert!(request.provider_options.is_none());
}

// =========================================================================
// P15: top_k passthrough from ModelInfo
// =========================================================================

#[test]
fn test_top_k_from_model_info() {
    let spec = ModelSpec::new("gemini", "gemini-2.5-pro");
    let info = ModelInfo {
        slug: "gemini-2.5-pro".to_string(),
        context_window: Some(1000000),
        max_output_tokens: Some(65536),
        top_k: Some(64),
        ..Default::default()
    };

    let ctx = InferenceContext::new(
        "call-1",
        "session-1",
        1,
        spec,
        info,
        AgentKind::Main,
        ExecutionIdentity::main(),
    );

    let messages = vec![LanguageModelMessage::user_text("Hello")];
    let request = RequestBuilder::new(ctx).messages(messages).build();

    assert_eq!(request.top_k, Some(64));
}

#[test]
fn test_top_k_none_by_default() {
    let ctx = sample_context(); // No top_k in ModelInfo
    let messages = vec![LanguageModelMessage::user_text("Hello")];

    let request = RequestBuilder::new(ctx).messages(messages).build();

    assert_eq!(request.top_k, None);
}
