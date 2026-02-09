use super::*;
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
    let messages = vec![Message::user("Hello")];

    let request = RequestBuilder::new(ctx).messages(messages).build();

    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.temperature, Some(1.0));
    // top_p comparison with tolerance due to f32->f64 conversion
    assert!((request.top_p.unwrap() - 0.9).abs() < 0.001);
    assert_eq!(request.max_tokens, Some(16384));
}

#[test]
fn test_override_temperature() {
    let ctx = sample_context();
    let messages = vec![Message::user("Hello")];

    let request = RequestBuilder::new(ctx)
        .messages(messages)
        .temperature(0.5)
        .build();

    assert_eq!(request.temperature, Some(0.5));
}

#[test]
fn test_override_max_tokens() {
    let ctx = sample_context();
    let messages = vec![Message::user("Hello")];

    let request = RequestBuilder::new(ctx)
        .messages(messages)
        .max_tokens(1000)
        .build();

    assert_eq!(request.max_tokens, Some(1000));
}

#[test]
fn test_with_tools() {
    let ctx = sample_context();
    let messages = vec![Message::user("Hello")];
    let tools = vec![ToolDefinition {
        name: "test_tool".to_string(),
        description: Some("A test tool".to_string()),
        parameters: serde_json::json!({"type": "object"}),
        custom_format: None,
    }];

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

    let messages = vec![Message::user("Hello")];
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
    let messages = vec![Message::user("Hello")];

    let request = build_request(ctx, messages, None);

    assert_eq!(request.messages.len(), 1);
    assert!(request.tools.is_none());
}
