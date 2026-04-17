use coco_types::AssistantContent;
use coco_types::AssistantMessage;
use coco_types::LlmMessage;
use coco_types::Message;
use uuid::Uuid;

use super::*;

fn make_assistant_with_tool_call(tool_name: &str, input_json: &str) -> Message {
    let input: serde_json::Value =
        serde_json::from_str(input_json).expect("test input should be valid JSON");
    Message::Assistant(AssistantMessage {
        message: LlmMessage::Assistant {
            content: vec![
                AssistantContent::Text(vercel_ai_provider::TextPart::new(
                    "Let me run a tool".to_string(),
                )),
                AssistantContent::ToolCall(vercel_ai_provider::ToolCallPart {
                    tool_call_id: format!("call_{tool_name}"),
                    tool_name: tool_name.to_string(),
                    input,
                    provider_executed: None,
                    provider_metadata: None,
                }),
            ],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        model: "test".to_string(),
        stop_reason: None,
        usage: None,
        cost_usd: None,
        request_id: None,
        api_error: None,
    })
}

fn make_assistant_with_thinking(text: &str, thinking: &str) -> Message {
    Message::Assistant(AssistantMessage {
        message: LlmMessage::Assistant {
            content: vec![
                AssistantContent::Reasoning(vercel_ai_provider::ReasoningPart {
                    text: thinking.to_string(),
                    provider_metadata: None,
                }),
                AssistantContent::Text(vercel_ai_provider::TextPart::new(text.to_string())),
            ],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        model: "test".to_string(),
        stop_reason: None,
        usage: None,
        cost_usd: None,
        request_id: None,
        api_error: None,
    })
}

fn make_assistant_text(text: &str) -> Message {
    Message::Assistant(AssistantMessage {
        message: LlmMessage::Assistant {
            content: vec![AssistantContent::Text(vercel_ai_provider::TextPart::new(
                text.to_string(),
            ))],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        model: "test".to_string(),
        stop_reason: None,
        usage: None,
        cost_usd: None,
        request_id: None,
        api_error: None,
    })
}

// --- clear_tool_uses tests ---

#[test]
fn test_clear_tool_uses_clears_old_inputs() {
    let big_input = format!(r#"{{"code": "{}"}}"#, "x".repeat(500));
    let mut messages = vec![
        make_assistant_with_tool_call("Bash", &big_input),
        make_assistant_with_tool_call("Read", &big_input),
        make_assistant_with_tool_call("Edit", &big_input),
    ];

    let result = clear_tool_uses(&mut messages, /*keep_recent_count*/ 1, &[]);
    assert!(
        result.messages_cleared >= 2,
        "should clear tool inputs from at least 2 old messages"
    );
    assert!(result.tokens_saved_estimate > 0);

    // Recent message should be untouched
    if let Message::Assistant(asst) = &messages[2]
        && let LlmMessage::Assistant { content, .. } = &asst.message
    {
        for part in content {
            if let AssistantContent::ToolCall(tc) = part {
                assert_ne!(
                    tc.input,
                    serde_json::Value::Object(serde_json::Map::new()),
                    "recent tool call input should be preserved"
                );
            }
        }
    }
}

#[test]
fn test_clear_tool_uses_respects_exclude() {
    let big_input = format!(r#"{{"code": "{}"}}"#, "x".repeat(500));
    let mut messages = vec![make_assistant_with_tool_call("Read", &big_input)];

    let result = clear_tool_uses(
        &mut messages,
        /*keep_recent_count*/ 0,
        &["Read".to_string()],
    );
    assert_eq!(
        result.messages_cleared, 0,
        "excluded tool should not be cleared"
    );
}

#[test]
fn test_clear_tool_uses_skips_tiny_inputs() {
    let mut messages = vec![make_assistant_with_tool_call("Bash", r#"{"x": 1}"#)];

    let result = clear_tool_uses(&mut messages, /*keep_recent_count*/ 0, &[]);
    assert_eq!(
        result.messages_cleared, 0,
        "tiny inputs should not be cleared"
    );
}

#[test]
fn test_clear_tool_uses_no_assistant_messages() {
    let mut messages: Vec<Message> = vec![];
    let result = clear_tool_uses(&mut messages, /*keep_recent_count*/ 0, &[]);
    assert_eq!(result.messages_cleared, 0);
    assert_eq!(result.tokens_saved_estimate, 0);
}

// --- clear_thinking tests ---

#[test]
fn test_clear_thinking_removes_all_reasoning() {
    let long_thinking = "Let me reason through this step by step. ".repeat(50);
    let mut messages = vec![
        make_assistant_with_thinking("Response 1", &long_thinking),
        make_assistant_with_thinking("Response 2", &long_thinking),
    ];

    let result = clear_thinking(&mut messages);
    assert_eq!(
        result.messages_cleared, 2,
        "should remove reasoning from both messages"
    );
    assert!(result.tokens_saved_estimate > 0);

    // Verify reasoning is gone but text remains
    for msg in &messages {
        if let Message::Assistant(asst) = msg
            && let LlmMessage::Assistant { content, .. } = &asst.message
        {
            let has_reasoning = content
                .iter()
                .any(|c| matches!(c, AssistantContent::Reasoning(_)));
            assert!(!has_reasoning, "reasoning blocks should be removed");
            let has_text = content
                .iter()
                .any(|c| matches!(c, AssistantContent::Text(_)));
            assert!(has_text, "text parts should be preserved");
        }
    }
}

#[test]
fn test_clear_thinking_no_op_without_reasoning() {
    let mut messages = vec![make_assistant_text("Just text, no thinking")];
    let result = clear_thinking(&mut messages);
    assert_eq!(result.messages_cleared, 0);
    assert_eq!(result.tokens_saved_estimate, 0);
}

#[test]
fn test_clear_thinking_empty_messages() {
    let mut messages: Vec<Message> = vec![];
    let result = clear_thinking(&mut messages);
    assert_eq!(result.messages_cleared, 0);
    assert_eq!(result.tokens_saved_estimate, 0);
}
