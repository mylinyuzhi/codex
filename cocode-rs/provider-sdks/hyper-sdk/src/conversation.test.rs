use super::*;
use crate::messages::ContentBlock;
use crate::response::FinishReason;

#[test]
fn test_conversation_context_new() {
    let ctx = ConversationContext::new();
    assert!(!ctx.id().is_empty());
    assert!(ctx.messages().is_empty());
    assert!(ctx.previous_response_id().is_none());
}

#[test]
fn test_conversation_context_with_id() {
    let ctx = ConversationContext::with_id("my-conversation");
    assert_eq!(ctx.id(), "my-conversation");
}

#[test]
fn test_message_history() {
    let mut ctx = ConversationContext::new();
    assert!(ctx.messages().is_empty());

    ctx.add_message(Message::user("Hello"));
    ctx.add_message(Message::assistant("Hi there!"));

    assert_eq!(ctx.messages().len(), 2);

    ctx.clear_history();
    assert!(ctx.messages().is_empty());
}

#[test]
fn test_previous_response_id() {
    let mut ctx = ConversationContext::new();
    assert!(ctx.previous_response_id().is_none());

    ctx.set_previous_response_id("resp_123");
    assert_eq!(ctx.previous_response_id(), Some("resp_123"));

    ctx.clear_previous_response_id();
    assert!(ctx.previous_response_id().is_none());
}

#[test]
fn test_session_config_integration() {
    let config = SessionConfig::new().temperature(0.7).max_tokens(4096);

    let ctx = ConversationContext::new().with_session_config(config);

    assert_eq!(ctx.session_config().temperature, Some(0.7));
    assert_eq!(ctx.session_config().max_tokens, Some(4096));
}

#[test]
fn test_builder() {
    let ctx = ConversationContextBuilder::new()
        .id("conv_123")
        .provider("openai")
        .model_id("gpt-4o")
        .session_config(SessionConfig::new().temperature(0.5))
        .without_history()
        .build();

    assert_eq!(ctx.id(), "conv_123");
    assert_eq!(ctx.provider, "openai");
    assert_eq!(ctx.model_id, "gpt-4o");
    assert_eq!(ctx.session_config().temperature, Some(0.5));
    assert!(!ctx.track_history);
}

#[tokio::test]
async fn test_process_response_updates_state() {
    let mut ctx = ConversationContext::new().with_provider_info("openai", "gpt-4o");

    let mut response = GenerateResponse::new("resp_123", "gpt-4o")
        .with_content(vec![ContentBlock::text("Hello!")])
        .with_finish_reason(FinishReason::Stop);

    let hook_ctx = HookContext::with_provider("openai", "gpt-4o");

    ctx.process_response(&mut response, &hook_ctx)
        .await
        .unwrap();

    // Previous response ID should be updated
    assert_eq!(ctx.previous_response_id(), Some("resp_123"));

    // Message should be added to history
    assert_eq!(ctx.messages().len(), 1);
    assert_eq!(ctx.messages()[0].role, Role::Assistant);

    // Source metadata should be set from hook context
    assert_eq!(
        ctx.messages()[0].metadata.source_provider,
        Some("openai".to_string()),
        "Source provider should be set from hook context"
    );
    assert_eq!(
        ctx.messages()[0].metadata.source_model,
        Some("gpt-4o".to_string()),
        "Source model should be set from hook context"
    );
}

// ============================================================
// Auto-Attach History Tests
// ============================================================

#[derive(Debug)]
struct MockModel {
    provider: String,
    model_id: String,
}

impl MockModel {
    fn new(provider: &str, model_id: &str) -> Self {
        Self {
            provider: provider.to_string(),
            model_id: model_id.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl crate::model::Model for MockModel {
    fn model_name(&self) -> &str {
        &self.model_id
    }
    fn provider(&self) -> &str {
        &self.provider
    }
    async fn generate(
        &self,
        _request: GenerateRequest,
    ) -> Result<crate::response::GenerateResponse, HyperError> {
        Ok(
            crate::response::GenerateResponse::new("resp_1", &self.model_id)
                .with_content(vec![ContentBlock::text("Response")])
                .with_finish_reason(FinishReason::Stop),
        )
    }
    async fn stream(
        &self,
        _request: GenerateRequest,
    ) -> Result<crate::stream::StreamResponse, HyperError> {
        Err(HyperError::UnsupportedCapability("streaming".to_string()))
    }
}

#[tokio::test]
async fn test_prepare_request_with_history() {
    let mock_model = MockModel::new("openai", "gpt-4o");
    let mut ctx = ConversationContext::new();

    // Add some history
    ctx.add_message(Message::user("Previous question"));
    ctx.add_message(Message::assistant("Previous answer"));

    // Create a new request
    let request = GenerateRequest::new(vec![Message::user("New question")]);

    // Prepare with history (default)
    let (prepared, _) = ctx
        .prepare_request(request, &mock_model, true)
        .await
        .unwrap();

    // Should have 3 messages: history (2) + new (1)
    assert_eq!(prepared.messages.len(), 3);
    assert_eq!(prepared.messages[0].text(), "Previous question");
    assert_eq!(prepared.messages[1].text(), "Previous answer");
    assert_eq!(prepared.messages[2].text(), "New question");
}

#[tokio::test]
async fn test_prepare_request_without_history() {
    let mock_model = MockModel::new("openai", "gpt-4o");
    let mut ctx = ConversationContext::new();

    // Add some history
    ctx.add_message(Message::user("Previous question"));
    ctx.add_message(Message::assistant("Previous answer"));

    // Create a new request
    let request = GenerateRequest::new(vec![Message::user("New question")]);

    // Prepare without history
    let (prepared, _) = ctx
        .prepare_request(request, &mock_model, false)
        .await
        .unwrap();

    // Should only have the new message
    assert_eq!(prepared.messages.len(), 1);
    assert_eq!(prepared.messages[0].text(), "New question");
}

#[tokio::test]
async fn test_generate_auto_attaches_history() {
    let mock_model = MockModel::new("openai", "gpt-4o");
    let mut ctx = ConversationContext::new();

    // First turn
    let _response = ctx
        .generate(
            &mock_model,
            GenerateRequest::new(vec![Message::user("Hello")]),
        )
        .await
        .unwrap();

    // History should contain user message + assistant response
    assert_eq!(ctx.messages().len(), 2);

    // Second turn - should automatically include history
    let _response = ctx
        .generate(
            &mock_model,
            GenerateRequest::new(vec![Message::user("Follow up")]),
        )
        .await
        .unwrap();

    // History should now have 4 messages
    assert_eq!(ctx.messages().len(), 4);
}

#[tokio::test]
async fn test_generate_stateless_no_history() {
    let mock_model = MockModel::new("openai", "gpt-4o");
    let mut ctx = ConversationContext::new();

    // Add some history
    ctx.add_message(Message::user("Previous question"));
    ctx.add_message(Message::assistant("Previous answer"));

    // Use generate_stateless - should NOT prepend history
    let _response = ctx
        .generate_stateless(
            &mock_model,
            GenerateRequest::new(vec![Message::user("Independent")]),
        )
        .await
        .unwrap();

    // The new user message should still be tracked
    assert_eq!(ctx.messages().len(), 4); // 2 original + 1 new user + 1 response
}

// ============================================================
// Cross-Provider switch_provider Tests
// ============================================================

#[test]
fn test_switch_provider_sanitizes_history() {
    let mut ctx = ConversationContext::new().with_provider_info("openai", "gpt-4o");

    // Add OpenAI message to history
    ctx.add_message(Message::user("Hello"));
    ctx.add_message(Message::assistant("Hi from OpenAI!").with_source("openai", "gpt-4o"));

    // Add Anthropic message with thinking signature
    let anthropic_msg = Message::new(
        Role::Assistant,
        vec![
            ContentBlock::Thinking {
                content: "Thinking from Anthropic".to_string(),
                signature: Some("anthropic-signature-xyz".to_string()),
            },
            ContentBlock::text("Response from Anthropic"),
        ],
    )
    .with_source("anthropic", "claude-sonnet-4-20250514");
    ctx.add_message(anthropic_msg);

    // Set previous_response_id (OpenAI-specific)
    ctx.set_previous_response_id("resp_123");

    // Switch to Gemini
    ctx.switch_provider("gemini", "gemini-2.5-pro");

    // Verify: previous_response_id cleared (OpenAI-specific, meaningless to Gemini)
    assert!(
        ctx.previous_response_id().is_none(),
        "previous_response_id should be cleared when switching providers"
    );

    // Verify: Provider updated
    assert_eq!(ctx.provider(), "gemini");
    assert_eq!(ctx.model_id(), "gemini-2.5-pro");

    // Verify: Thinking signatures stripped from history
    for msg in ctx.messages() {
        for block in &msg.content {
            if let ContentBlock::Thinking { signature, .. } = block {
                assert!(
                    signature.is_none(),
                    "Thinking signatures should be stripped when switching providers"
                );
            }
        }
    }

    // Verify: Source tracking preserved (for debugging)
    assert_eq!(
        ctx.messages()[1].metadata.source_provider,
        Some("openai".to_string())
    );
    assert_eq!(
        ctx.messages()[2].metadata.source_provider,
        Some("anthropic".to_string())
    );
}

#[test]
fn test_switch_provider_clears_provider_options() {
    use crate::options::OpenAIOptions;

    let mut ctx = ConversationContext::new().with_provider_info("openai", "gpt-4o");

    // Add message with OpenAI-specific options
    let openai_opts: crate::options::ProviderOptions = Box::new(OpenAIOptions {
        previous_response_id: Some("resp_prev".to_string()),
        ..Default::default()
    });
    let msg = Message::assistant("Response")
        .with_source("openai", "gpt-4o")
        .with_provider_options(openai_opts);
    ctx.add_message(msg);

    // Switch to Anthropic
    ctx.switch_provider("anthropic", "claude-sonnet-4-20250514");

    // Verify: Provider options cleared (OpenAI options don't apply to Anthropic)
    assert!(
        ctx.messages()[0].provider_options.is_none(),
        "Provider options should be cleared when switching providers"
    );
}

#[test]
fn test_switch_provider_preserves_tool_call_ids() {
    let mut ctx = ConversationContext::new().with_provider_info("openai", "gpt-4o");

    // Add tool call from OpenAI
    let tool_call = Message::new(
        Role::Assistant,
        vec![ContentBlock::tool_use(
            "call_001",
            "get_weather",
            serde_json::json!({"city": "NYC"}),
        )],
    )
    .with_source("openai", "gpt-4o");
    ctx.add_message(tool_call);

    // Add tool result
    ctx.add_message(Message::tool_result(
        "call_001",
        crate::tools::ToolResultContent::text("Weather: Sunny"),
    ));

    // Switch to Anthropic
    ctx.switch_provider("anthropic", "claude-sonnet-4-20250514");

    // Verify: Tool call ID preserved
    if let ContentBlock::ToolUse { id, name, .. } = &ctx.messages()[0].content[0] {
        assert_eq!(id, "call_001", "Tool call ID must be preserved");
        assert_eq!(name, "get_weather");
    } else {
        panic!("Expected ToolUse block");
    }

    // Verify: Tool result ID preserved
    if let ContentBlock::ToolResult { tool_use_id, .. } = &ctx.messages()[1].content[0] {
        assert_eq!(tool_use_id, "call_001", "Tool result ID must match");
    } else {
        panic!("Expected ToolResult block");
    }
}
