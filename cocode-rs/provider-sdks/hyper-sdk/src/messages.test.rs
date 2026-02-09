use super::*;

#[test]
fn test_message_constructors() {
    let user_msg = Message::user("Hello!");
    assert_eq!(user_msg.role, Role::User);
    assert_eq!(user_msg.text(), "Hello!");

    let assistant_msg = Message::assistant("Hi there!");
    assert_eq!(assistant_msg.role, Role::Assistant);
    assert_eq!(assistant_msg.text(), "Hi there!");

    let system_msg = Message::system("You are helpful.");
    assert_eq!(system_msg.role, Role::System);
}

#[test]
fn test_user_with_image() {
    let msg = Message::user_with_image(
        "What's in this image?",
        ImageSource::Url {
            url: "https://example.com/image.png".to_string(),
        },
    );

    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.content.len(), 2);
    assert!(msg.content[0].as_text().is_some());
    assert!(matches!(msg.content[1], ContentBlock::Image { .. }));
}

#[test]
fn test_content_block_serde() {
    let block = ContentBlock::text("Hello");
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("\"type\":\"text\""));

    let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.as_text(), Some("Hello"));
}

#[test]
fn test_tool_use_block() {
    let block = ContentBlock::tool_use(
        "call_123",
        "get_weather",
        serde_json::json!({"location": "NYC"}),
    );

    assert!(block.is_tool_use());
    assert!(!block.is_thinking());
}

// ============================================================
// Cross-Provider Tests
// ============================================================

#[test]
fn test_provider_metadata_new() {
    let meta = ProviderMetadata::new();
    assert!(meta.source_provider.is_none());
    assert!(meta.source_model.is_none());
    assert!(meta.extensions.is_empty());
    assert!(meta.is_empty());
}

#[test]
fn test_provider_metadata_with_source() {
    let meta = ProviderMetadata::with_source("openai", "gpt-4o");
    assert_eq!(meta.source_provider, Some("openai".to_string()));
    assert_eq!(meta.source_model, Some("gpt-4o".to_string()));
    assert!(!meta.is_empty());
}

#[test]
fn test_provider_metadata_is_from() {
    let meta = ProviderMetadata::with_source("openai", "gpt-4o");
    assert!(meta.is_from_provider("openai"));
    assert!(!meta.is_from_provider("anthropic"));
    assert!(meta.is_from("openai", "gpt-4o"));
    assert!(!meta.is_from("openai", "gpt-4o-mini"));
}

#[test]
fn test_provider_metadata_extensions() {
    let mut meta = ProviderMetadata::new();
    meta.set_extension("openai", serde_json::json!({"cache_hit": true}));

    assert!(meta.get_extension("openai").is_some());
    assert!(meta.get_extension("anthropic").is_none());

    let removed = meta.remove_extension("openai");
    assert!(removed.is_some());
    assert!(meta.get_extension("openai").is_none());
}

#[test]
fn test_message_with_source() {
    let msg = Message::assistant("Response from OpenAI").with_source("openai", "gpt-4o");

    assert_eq!(msg.source_provider(), Some("openai"));
    assert_eq!(msg.source_model(), Some("gpt-4o"));
    assert!(msg.metadata.is_from_provider("openai"));
}

#[test]
fn test_openai_history_to_anthropic() {
    // Create messages that came from OpenAI
    let mut openai_msg =
        Message::assistant("I can help with that.").with_source("openai", "gpt-4o");

    // Add thinking block (no signature from OpenAI)
    openai_msg.content.push(ContentBlock::Thinking {
        content: "Let me think...".to_string(),
        signature: None,
    });

    // Convert for Anthropic
    openai_msg.convert_for_provider("anthropic", "claude-sonnet-4-20250514");

    // Source tracking should be preserved
    assert_eq!(
        openai_msg.metadata.source_provider,
        Some("openai".to_string())
    );
    // Provider options should be cleared
    assert!(openai_msg.provider_options.is_none());
}

#[test]
fn test_anthropic_thinking_to_openai() {
    // Create message with Claude thinking signature
    let mut claude_msg = Message::new(
        Role::Assistant,
        vec![
            ContentBlock::Thinking {
                content: "Deep reasoning here...".to_string(),
                signature: Some("base64-encrypted-signature-from-claude".to_string()),
            },
            ContentBlock::text("The answer is 42."),
        ],
    )
    .with_source("anthropic", "claude-sonnet-4-20250514");

    // Convert for OpenAI
    claude_msg.convert_for_provider("openai", "gpt-4o");

    // Signature should be stripped
    if let ContentBlock::Thinking { signature, content } = &claude_msg.content[0] {
        assert!(
            signature.is_none(),
            "Signature should be stripped for OpenAI"
        );
        assert_eq!(content, "Deep reasoning here...");
    } else {
        panic!("Expected Thinking block");
    }
}

#[test]
fn test_tool_calls_cross_provider() {
    // OpenAI tool call
    let openai_tool_msg = Message::new(
        Role::Assistant,
        vec![ContentBlock::tool_use(
            "call_abc123",
            "get_weather",
            serde_json::json!({"location": "NYC"}),
        )],
    )
    .with_source("openai", "gpt-4o");

    // Tool result
    let tool_result = Message::tool_result(
        "call_abc123",
        crate::tools::ToolResultContent::text("Weather: Sunny, 72°F"),
    );

    let mut history = vec![
        Message::user("What's the weather in NYC?"),
        openai_tool_msg,
        tool_result,
    ];

    // Convert for Anthropic
    for msg in &mut history {
        msg.convert_for_provider("anthropic", "claude-3-opus");
    }

    // ToolUse/ToolResult structure should be preserved
    if let ContentBlock::ToolUse { id, name, .. } = &history[1].content[0] {
        assert_eq!(id, "call_abc123");
        assert_eq!(name, "get_weather");
    } else {
        panic!("Expected ToolUse block");
    }
}

#[test]
fn test_provider_options_handling() {
    // Message with OpenAI-specific options
    let openai_opts: crate::options::ProviderOptions =
        Box::new(crate::options::OpenAIOptions {
            previous_response_id: Some("resp_123".to_string()),
            ..Default::default()
        });

    let mut msg = Message::assistant("Response")
        .with_source("openai", "gpt-4o")
        .with_provider_options(openai_opts);

    // Convert for Anthropic - options should be cleared
    msg.convert_for_provider("anthropic", "claude-3-opus");
    assert!(msg.provider_options.is_none());

    // Same provider, different model - options should be preserved
    let openai_opts2: crate::options::ProviderOptions =
        Box::new(crate::options::OpenAIOptions::default());
    let mut msg2 = Message::assistant("Response")
        .with_source("openai", "gpt-4o")
        .with_provider_options(openai_opts2);
    msg2.convert_for_provider("openai", "gpt-4o-mini");
    assert!(msg2.provider_options.is_some()); // Same provider, preserved
}

#[test]
fn test_multi_turn_cross_provider() {
    // Simulate: User -> OpenAI -> User -> Anthropic -> User -> Gemini
    let mut conversation = vec![
        // Turn 1: User to OpenAI
        Message::user("Explain quantum computing"),
        Message::assistant("Quantum computing uses qubits...").with_source("openai", "gpt-4o"),
        // Turn 2: User to Anthropic (with OpenAI history)
        Message::user("Can you elaborate on superposition?"),
        Message::new(
            Role::Assistant,
            vec![
                ContentBlock::Thinking {
                    content: "The user wants details on superposition...".to_string(),
                    signature: Some("anthropic-sig-xyz".to_string()),
                },
                ContentBlock::text("Superposition is a quantum principle where..."),
            ],
        )
        .with_source("anthropic", "claude-sonnet-4-20250514"),
    ];

    // Convert all for Gemini
    for msg in &mut conversation {
        msg.convert_for_provider("gemini", "gemini-1.5-pro");
    }

    // All thinking signatures should be stripped
    for msg in &conversation {
        for block in &msg.content {
            if let ContentBlock::Thinking { signature, .. } = block {
                assert!(
                    signature.is_none(),
                    "All thinking signatures should be stripped for Gemini"
                );
            }
        }
    }

    // Source tracking should be preserved (for debugging)
    assert_eq!(
        conversation[1].metadata.source_provider,
        Some("openai".to_string())
    );
    assert_eq!(
        conversation[3].metadata.source_provider,
        Some("anthropic".to_string())
    );
}

#[test]
fn test_sanitize_for_target() {
    let mut msg = Message::new(
        Role::Assistant,
        vec![
            ContentBlock::Thinking {
                content: "Thinking content".to_string(),
                signature: Some("sig".to_string()),
            },
            ContentBlock::text("Response"),
        ],
    )
    .with_source("anthropic", "claude-sonnet-4-20250514");

    // Same provider/model - signature preserved
    msg.sanitize_for_target("anthropic", "claude-sonnet-4-20250514");
    if let ContentBlock::Thinking { signature, .. } = &msg.content[0] {
        assert!(
            signature.is_some(),
            "Signature should be preserved for same provider/model"
        );
    }

    // Different model - signature stripped
    msg.sanitize_for_target("anthropic", "claude-opus-4-20250514");
    if let ContentBlock::Thinking { signature, .. } = &msg.content[0] {
        assert!(
            signature.is_none(),
            "Signature should be stripped for different model"
        );
    }
}

// ============================================================
// Cross-Provider Integration Tests (from design document)
// ============================================================

/// Test: OpenAI-generated history with tool calls can be sent to Anthropic.
/// Verifies that ToolUse/ToolResult IDs are preserved across providers.
#[test]
fn test_openai_tool_history_to_anthropic() {
    // Build OpenAI response history with tool call
    let openai_response = Message::new(
        Role::Assistant,
        vec![
            ContentBlock::text("I'll help you with that task."),
            ContentBlock::tool_use(
                "call_abc123",
                "read_file",
                serde_json::json!({"path": "/tmp/test.txt"}),
            ),
        ],
    )
    .with_source("openai", "gpt-4o");

    let tool_result =
        Message::tool_result("call_abc123", ToolResultContent::text("File content here"));

    let mut history = vec![
        Message::user("Please read /tmp/test.txt"),
        openai_response,
        tool_result,
        Message::user("Now summarize it"),
    ];

    // Sanitize for Anthropic
    for msg in &mut history {
        msg.convert_for_provider("anthropic", "claude-sonnet-4-20250514");
    }

    // Verify: ToolUse ID preserved (critical for tool call correlation)
    if let ContentBlock::ToolUse { id, name, .. } = &history[1].content[1] {
        assert_eq!(
            id, "call_abc123",
            "ToolUse ID must be preserved across providers"
        );
        assert_eq!(name, "read_file");
    } else {
        panic!("Expected ToolUse block");
    }

    // Verify: ToolResult ID matches ToolUse ID
    if let ContentBlock::ToolResult { tool_use_id, .. } = &history[2].content[0] {
        assert_eq!(
            tool_use_id, "call_abc123",
            "ToolResult ID must match ToolUse ID"
        );
    } else {
        panic!("Expected ToolResult block");
    }

    // Verify: Source tracking preserved (for debugging)
    assert_eq!(
        history[1].metadata.source_provider,
        Some("openai".to_string())
    );
}

/// Test: Anthropic thinking with signature sent to OpenAI has signature stripped.
#[test]
fn test_anthropic_thinking_signature_to_openai() {
    let mut anthropic_response = Message::new(
        Role::Assistant,
        vec![
            ContentBlock::Thinking {
                content: "Let me analyze this step by step...".to_string(),
                signature: Some("base64-anthropic-signature-xyz".to_string()),
            },
            ContentBlock::text("Based on my analysis, the answer is 42."),
        ],
    )
    .with_source("anthropic", "claude-sonnet-4-20250514");

    // Sanitize for OpenAI
    anthropic_response.convert_for_provider("openai", "gpt-4o");

    // Verify: Signature stripped (OpenAI cannot verify Anthropic signatures)
    if let ContentBlock::Thinking { signature, content } = &anthropic_response.content[0] {
        assert!(signature.is_none(), "Signature must be stripped for OpenAI");
        assert_eq!(
            content, "Let me analyze this step by step...",
            "Thinking content preserved"
        );
    } else {
        panic!("Expected Thinking block");
    }

    // Verify: Text content preserved
    assert_eq!(
        anthropic_response.content[1].as_text(),
        Some("Based on my analysis, the answer is 42.")
    );
}

/// Test: Multi-hop provider switching (OpenAI → Anthropic → Gemini → OpenAI).
#[test]
fn test_multi_hop_provider_conversation() {
    // Turn 1: OpenAI response
    let openai_msg = Message::assistant("OpenAI response").with_source("openai", "gpt-4o");

    // Turn 2: Anthropic response with thinking
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

    // Turn 3: Gemini response with thinking (no signature - Gemini doesn't use signatures)
    let gemini_msg = Message::new(
        Role::Assistant,
        vec![
            ContentBlock::Thinking {
                content: "Gemini thinking".to_string(),
                signature: None,
            },
            ContentBlock::text("Gemini response"),
        ],
    )
    .with_source("gemini", "gemini-2.5-pro");

    // Build full history
    let mut history = vec![
        Message::user("Question 1"),
        openai_msg,
        Message::user("Question 2"),
        anthropic_msg,
        Message::user("Question 3"),
        gemini_msg,
        Message::user("Question 4"),
    ];

    // Now switch back to OpenAI for the next turn
    for msg in &mut history {
        msg.convert_for_provider("openai", "gpt-4o");
    }

    // Verify: All thinking signatures stripped
    for msg in &history {
        for block in &msg.content {
            if let ContentBlock::Thinking { signature, .. } = block {
                assert!(
                    signature.is_none(),
                    "All signatures should be stripped for OpenAI"
                );
            }
        }
    }

    // Verify: Source tracking preserved for all assistant messages
    assert_eq!(
        history[1].metadata.source_provider,
        Some("openai".to_string())
    );
    assert_eq!(
        history[3].metadata.source_provider,
        Some("anthropic".to_string())
    );
    assert_eq!(
        history[5].metadata.source_provider,
        Some("gemini".to_string())
    );
}

/// Test: Tool call continuity across provider switch with follow-up.
#[test]
fn test_tool_call_continuity_across_providers() {
    // OpenAI makes a tool call
    let openai_tool_call = Message::new(
        Role::Assistant,
        vec![ContentBlock::tool_use(
            "call_001",
            "get_weather",
            serde_json::json!({"city": "NYC"}),
        )],
    )
    .with_source("openai", "gpt-4o");

    // User provides tool result
    let tool_result =
        Message::tool_result("call_001", ToolResultContent::text("Weather: Sunny, 72°F"));

    // OpenAI continues with tool result
    let openai_followup = Message::assistant("The weather in NYC is sunny and 72°F.")
        .with_source("openai", "gpt-4o");

    // Now switch to Anthropic for the next turn
    let mut history = vec![
        Message::user("What's the weather in NYC?"),
        openai_tool_call,
        tool_result,
        openai_followup,
        Message::user("What about tomorrow?"),
    ];

    for msg in &mut history {
        msg.convert_for_provider("anthropic", "claude-sonnet-4-20250514");
    }

    // Verify: ToolUse ID preserved
    if let ContentBlock::ToolUse { id, name, .. } = &history[1].content[0] {
        assert_eq!(id, "call_001");
        assert_eq!(name, "get_weather");
    } else {
        panic!("Expected ToolUse block");
    }

    // Verify: ToolResult ID matches
    if let ContentBlock::ToolResult { tool_use_id, .. } = &history[2].content[0] {
        assert_eq!(tool_use_id, "call_001");
    } else {
        panic!("Expected ToolResult block");
    }

    // Verify: Text content preserved
    assert_eq!(history[3].text(), "The weather in NYC is sunny and 72°F.");
}

/// Test: Same provider, different model sanitization.
/// Model-specific signatures should be stripped even within the same provider.
#[test]
fn test_same_provider_different_model_sanitization() {
    let mut claude_sonnet_msg = Message::new(
        Role::Assistant,
        vec![
            ContentBlock::Thinking {
                content: "Thinking from Sonnet".to_string(),
                signature: Some("sonnet-4-specific-signature".to_string()),
            },
            ContentBlock::text("Response from Sonnet"),
        ],
    )
    .with_source("anthropic", "claude-sonnet-4-20250514");

    // Sanitize for Claude Opus (same provider, different model)
    claude_sonnet_msg.sanitize_for_target("anthropic", "claude-opus-4-20250514");

    // Signature should be stripped (different models may have incompatible signatures)
    if let ContentBlock::Thinking { signature, content } = &claude_sonnet_msg.content[0] {
        assert!(
            signature.is_none(),
            "Signature must be stripped for different model"
        );
        assert_eq!(
            content, "Thinking from Sonnet",
            "Thinking content preserved"
        );
    } else {
        panic!("Expected Thinking block");
    }

    // Text content preserved
    assert_eq!(
        claude_sonnet_msg.content[1].as_text(),
        Some("Response from Sonnet")
    );
}
