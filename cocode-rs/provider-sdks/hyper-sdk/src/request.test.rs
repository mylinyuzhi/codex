use super::*;

#[test]
fn test_request_builder() {
    let request = GenerateRequest::from_text("Hello!")
        .temperature(0.7)
        .max_tokens(1000)
        .top_p(0.9);

    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.temperature, Some(0.7));
    assert_eq!(request.max_tokens, Some(1000));
    assert_eq!(request.top_p, Some(0.9));
}

#[test]
fn test_with_tools() {
    let request =
        GenerateRequest::from_text("What's the weather?").tools(vec![ToolDefinition::full(
            "get_weather",
            "Get weather",
            serde_json::json!({"type": "object"}),
        )]);

    assert!(request.has_tools());
}

#[test]
fn test_add_message() {
    let request = GenerateRequest::from_text("Hello")
        .add_message(Message::assistant("Hi!"))
        .add_message(Message::user("How are you?"));

    assert_eq!(request.messages.len(), 3);
}

// ============================================================
// Cross-Provider Request Sanitization Tests
// ============================================================

#[test]
fn test_request_sanitize_for_target_strips_signatures() {
    use crate::messages::ContentBlock;
    use crate::messages::Role;

    // Create request with messages from different providers
    let anthropic_msg = Message::new(
        Role::Assistant,
        vec![
            ContentBlock::Thinking {
                content: "Thinking content".to_string(),
                signature: Some("anthropic-signature".to_string()),
            },
            ContentBlock::text("Response"),
        ],
    )
    .with_source("anthropic", "claude-sonnet-4-20250514");

    let mut request = GenerateRequest::new(vec![
        Message::user("Hello"),
        anthropic_msg,
        Message::user("Follow up"),
    ]);

    // Sanitize for OpenAI
    request.sanitize_for_target("openai", "gpt-4o");

    // Verify: Signature stripped
    if let ContentBlock::Thinking { signature, content } = &request.messages[1].content[0] {
        assert!(signature.is_none(), "Signature should be stripped");
        assert_eq!(content, "Thinking content", "Content preserved");
    } else {
        panic!("Expected Thinking block");
    }
}

#[test]
fn test_request_strip_all_thinking_signatures() {
    use crate::messages::ContentBlock;
    use crate::messages::Role;

    let msg1 = Message::new(
        Role::Assistant,
        vec![ContentBlock::Thinking {
            content: "Thinking 1".to_string(),
            signature: Some("sig1".to_string()),
        }],
    )
    .with_source("anthropic", "claude-sonnet-4-20250514");

    let msg2 = Message::new(
        Role::Assistant,
        vec![ContentBlock::Thinking {
            content: "Thinking 2".to_string(),
            signature: Some("sig2".to_string()),
        }],
    )
    .with_source("openai", "gpt-4o");

    let mut request = GenerateRequest::new(vec![
        Message::user("Question"),
        msg1,
        Message::user("Follow up"),
        msg2,
    ]);

    // Strip all signatures regardless of source
    request.strip_all_thinking_signatures();

    // Verify: All signatures stripped
    for msg in &request.messages {
        for block in &msg.content {
            if let ContentBlock::Thinking { signature, .. } = block {
                assert!(signature.is_none(), "All signatures should be stripped");
            }
        }
    }
}

#[test]
fn test_request_sanitize_preserves_same_provider_model() {
    use crate::messages::ContentBlock;
    use crate::messages::Role;

    let anthropic_msg = Message::new(
        Role::Assistant,
        vec![ContentBlock::Thinking {
            content: "Thinking content".to_string(),
            signature: Some("sig".to_string()),
        }],
    )
    .with_source("anthropic", "claude-sonnet-4-20250514");

    let mut request = GenerateRequest::new(vec![Message::user("Hello"), anthropic_msg]);

    // Sanitize for same provider/model
    request.sanitize_for_target("anthropic", "claude-sonnet-4-20250514");

    // Verify: Signature preserved for same provider/model
    if let ContentBlock::Thinking { signature, .. } = &request.messages[1].content[0] {
        assert!(
            signature.is_some(),
            "Signature should be preserved for same provider/model"
        );
    }
}

#[test]
fn test_request_sanitize_multi_provider_history() {
    use crate::messages::ContentBlock;
    use crate::messages::Role;

    // Build conversation with multiple providers
    let openai_msg = Message::assistant("OpenAI response").with_source("openai", "gpt-4o");

    let anthropic_msg = Message::new(
        Role::Assistant,
        vec![
            ContentBlock::Thinking {
                content: "Anthropic thinking".to_string(),
                signature: Some("ant-sig".to_string()),
            },
            ContentBlock::text("Anthropic response"),
        ],
    )
    .with_source("anthropic", "claude-sonnet-4-20250514");

    let gemini_msg = Message::new(
        Role::Assistant,
        vec![ContentBlock::Thinking {
            content: "Gemini thinking".to_string(),
            signature: None,
        }],
    )
    .with_source("gemini", "gemini-2.5-pro");

    let mut request = GenerateRequest::new(vec![
        Message::user("Q1"),
        openai_msg,
        Message::user("Q2"),
        anthropic_msg,
        Message::user("Q3"),
        gemini_msg,
        Message::user("Q4"),
    ]);

    // Sanitize for a completely different provider (Volcengine)
    request.sanitize_for_target("volcengine", "doubao-pro");

    // Verify: All signatures stripped for target provider
    for msg in &request.messages {
        for block in &msg.content {
            if let ContentBlock::Thinking { signature, .. } = block {
                assert!(
                    signature.is_none(),
                    "Signatures should be stripped for different provider"
                );
            }
        }
    }
}

// ============================================================
// Typed Provider Options Tests
// ============================================================

#[test]
fn test_with_openai_options() {
    use crate::options::downcast_options;
    use crate::options::openai::ReasoningEffort;

    let request = GenerateRequest::from_text("Hello")
        .with_openai_options(OpenAIOptions::new().with_reasoning_effort(ReasoningEffort::High));

    assert!(request.provider_options.is_some());
    let opts = downcast_options::<OpenAIOptions>(request.provider_options.as_ref().unwrap());
    assert!(opts.is_some());
    assert_eq!(opts.unwrap().reasoning_effort, Some(ReasoningEffort::High));
}

#[test]
fn test_with_anthropic_options() {
    use crate::options::downcast_options;

    let request = GenerateRequest::from_text("Hello")
        .with_anthropic_options(AnthropicOptions::new().with_thinking_budget(10000));

    assert!(request.provider_options.is_some());
    let opts = downcast_options::<AnthropicOptions>(request.provider_options.as_ref().unwrap());
    assert!(opts.is_some());
    assert_eq!(opts.unwrap().thinking_budget_tokens, Some(10000));
}

#[test]
fn test_with_gemini_options() {
    use crate::options::downcast_options;
    use crate::options::gemini::ThinkingLevel;

    let request = GenerateRequest::from_text("Hello")
        .with_gemini_options(GeminiOptions::new().with_thinking_level(ThinkingLevel::High));

    assert!(request.provider_options.is_some());
    let opts = downcast_options::<GeminiOptions>(request.provider_options.as_ref().unwrap());
    assert!(opts.is_some());
    assert_eq!(opts.unwrap().thinking_level, Some(ThinkingLevel::High));
}

#[test]
fn test_with_volcengine_options() {
    use crate::options::downcast_options;

    let request = GenerateRequest::from_text("Hello")
        .with_volcengine_options(VolcengineOptions::new().with_thinking_budget(2048));

    assert!(request.provider_options.is_some());
    let opts = downcast_options::<VolcengineOptions>(request.provider_options.as_ref().unwrap());
    assert!(opts.is_some());
    assert_eq!(opts.unwrap().thinking_budget_tokens, Some(2048));
}

#[test]
fn test_with_zai_options() {
    use crate::options::downcast_options;

    let request = GenerateRequest::from_text("Hello")
        .with_zai_options(ZaiOptions::new().with_do_sample(true));

    assert!(request.provider_options.is_some());
    let opts = downcast_options::<ZaiOptions>(request.provider_options.as_ref().unwrap());
    assert!(opts.is_some());
    assert_eq!(opts.unwrap().do_sample, Some(true));
}

#[test]
fn test_with_provider_options_generic() {
    use crate::options::downcast_options;

    let request = GenerateRequest::from_text("Hello")
        .with_provider_options(OpenAIOptions::new().with_seed(42));

    assert!(request.provider_options.is_some());
    let opts = downcast_options::<OpenAIOptions>(request.provider_options.as_ref().unwrap());
    assert!(opts.is_some());
    assert_eq!(opts.unwrap().seed, Some(42));
}
