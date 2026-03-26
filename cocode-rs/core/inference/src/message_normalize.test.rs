use super::*;
use crate::AssistantContentPart;
use crate::LanguageModelMessage;
use crate::ToolContentPart;
use vercel_ai_provider::ToolResultPart;

#[test]
fn test_remove_empty_text_parts() {
    let mut prompt = vec![LanguageModelMessage::Assistant {
        content: vec![
            AssistantContentPart::text("hello"),
            AssistantContentPart::text(""),
            AssistantContentPart::text("world"),
        ],
        provider_options: None,
    }];

    normalize_prompt(&mut prompt, ProviderApi::Anthropic);

    match &prompt[0] {
        LanguageModelMessage::Assistant { content, .. } => {
            assert_eq!(content.len(), 2);
        }
        _ => panic!("expected assistant message"),
    }
}

#[test]
fn test_remove_empty_reasoning_parts() {
    let mut prompt = vec![LanguageModelMessage::Assistant {
        content: vec![
            AssistantContentPart::reasoning("thinking..."),
            AssistantContentPart::reasoning(""),
            AssistantContentPart::text("answer"),
        ],
        provider_options: None,
    }];

    normalize_prompt(&mut prompt, ProviderApi::Anthropic);

    match &prompt[0] {
        LanguageModelMessage::Assistant { content, .. } => {
            assert_eq!(content.len(), 2);
        }
        _ => panic!("expected assistant message"),
    }
}

#[test]
fn test_remove_empty_messages() {
    let mut prompt = vec![
        LanguageModelMessage::user_text("hello"),
        LanguageModelMessage::Assistant {
            content: vec![],
            provider_options: None,
        },
        LanguageModelMessage::user_text("world"),
    ];

    normalize_prompt(&mut prompt, ProviderApi::Anthropic);

    assert_eq!(prompt.len(), 2);
    assert!(matches!(prompt[0], LanguageModelMessage::User { .. }));
    assert!(matches!(prompt[1], LanguageModelMessage::User { .. }));
}

#[test]
fn test_remove_messages_after_filtering_parts() {
    let mut prompt = vec![
        LanguageModelMessage::user_text("hello"),
        // This assistant message has only empty parts; after filtering parts it becomes empty
        LanguageModelMessage::Assistant {
            content: vec![AssistantContentPart::text("")],
            provider_options: None,
        },
        LanguageModelMessage::user_text("world"),
    ];

    normalize_prompt(&mut prompt, ProviderApi::Anthropic);

    // Empty text part removed, then empty assistant message removed
    assert_eq!(prompt.len(), 2);
}

#[test]
fn test_sanitize_tool_call_ids() {
    let mut prompt = vec![
        LanguageModelMessage::Assistant {
            content: vec![AssistantContentPart::tool_call(
                "call:123.abc!",
                "test_tool",
                serde_json::json!({}),
            )],
            provider_options: None,
        },
        LanguageModelMessage::Tool {
            content: vec![ToolContentPart::ToolResult(ToolResultPart::new(
                "call:123.abc!",
                "test_tool",
                crate::ToolResultContent::text("ok"),
            ))],
            provider_options: None,
        },
    ];

    normalize_prompt(&mut prompt, ProviderApi::Anthropic);

    match &prompt[0] {
        LanguageModelMessage::Assistant { content, .. } => {
            if let AssistantContentPart::ToolCall(tc) = &content[0] {
                assert_eq!(tc.tool_call_id, "call_123_abc_");
            } else {
                panic!("expected tool call");
            }
        }
        _ => panic!("expected assistant message"),
    }

    match &prompt[1] {
        LanguageModelMessage::Tool { content, .. } => {
            if let ToolContentPart::ToolResult(result) = &content[0] {
                assert_eq!(result.tool_call_id, "call_123_abc_");
            } else {
                panic!("expected tool result");
            }
        }
        _ => panic!("expected tool message"),
    }
}

#[test]
fn test_valid_ids_unchanged() {
    let mut prompt = vec![LanguageModelMessage::Assistant {
        content: vec![AssistantContentPart::tool_call(
            "toolu_abc-123_XYZ",
            "test_tool",
            serde_json::json!({}),
        )],
        provider_options: None,
    }];

    normalize_prompt(&mut prompt, ProviderApi::Anthropic);

    match &prompt[0] {
        LanguageModelMessage::Assistant { content, .. } => {
            if let AssistantContentPart::ToolCall(tc) = &content[0] {
                assert_eq!(tc.tool_call_id, "toolu_abc-123_XYZ");
            } else {
                panic!("expected tool call");
            }
        }
        _ => panic!("expected assistant message"),
    }
}

#[test]
fn test_non_anthropic_provider_no_changes() {
    let mut prompt = vec![LanguageModelMessage::Assistant {
        content: vec![
            AssistantContentPart::text(""),
            AssistantContentPart::tool_call("call:123", "test_tool", serde_json::json!({})),
        ],
        provider_options: None,
    }];

    normalize_prompt(&mut prompt, ProviderApi::Openai);

    // Nothing should change for non-Anthropic providers
    match &prompt[0] {
        LanguageModelMessage::Assistant { content, .. } => {
            assert_eq!(content.len(), 2); // empty text part preserved
            if let AssistantContentPart::ToolCall(tc) = &content[1] {
                assert_eq!(tc.tool_call_id, "call:123"); // not sanitized
            }
        }
        _ => panic!("expected assistant message"),
    }
}
