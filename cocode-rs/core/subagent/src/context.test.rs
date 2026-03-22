use super::*;

use cocode_api::AssistantContentPart;
use cocode_api::ToolCallPart;
use cocode_message::TrackedMessage;
use cocode_protocol::ToolName;

/// Helper: create an assistant message containing a single tool_use block.
fn assistant_with_tool_use(tool_call_id: &str, turn_id: &str) -> TrackedMessage {
    let part = AssistantContentPart::ToolCall(ToolCallPart::new(
        tool_call_id,
        ToolName::Bash.as_str(),
        serde_json::json!({}),
    ));
    cocode_message::create_assistant_message_with_content(vec![part], turn_id, None)
}

/// Helper: create a Tool result message for a given tool_call_id.
fn tool_result(tool_call_id: &str, turn_id: &str) -> TrackedMessage {
    cocode_message::create_tool_result_message(tool_call_id, "ok", turn_id)
}

#[test]
fn test_child_context_serde_roundtrip() {
    let ctx = ChildToolUseContext {
        parent_session_id: "parent-123".to_string(),
        child_session_id: "child-456".to_string(),
        forked_from_turn: 7,
    };
    let json = serde_json::to_string(&ctx).expect("serialize");
    let back: ChildToolUseContext = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.parent_session_id, "parent-123");
    assert_eq!(back.child_session_id, "child-456");
    assert_eq!(back.forked_from_turn, 7);
}

#[test]
fn test_filter_empty_messages() {
    let result = filter_orphaned_tool_uses(&[]);
    assert!(result.is_empty());
}

#[test]
fn test_filter_no_tool_use_preserves_all() {
    let messages = vec![
        TrackedMessage::user("hello", "t1"),
        TrackedMessage::assistant("hi there", "t1", None),
        TrackedMessage::user("thanks", "t2"),
    ];
    let result = filter_orphaned_tool_uses(&messages);
    assert_eq!(result.len(), 3);
}

#[test]
fn test_filter_resolved_tool_use_preserved() {
    let messages = vec![
        TrackedMessage::user("run ls", "t1"),
        assistant_with_tool_use("call-1", "t1"),
        tool_result("call-1", "t1"),
    ];
    let result = filter_orphaned_tool_uses(&messages);
    assert_eq!(result.len(), 3);
}

#[test]
fn test_filter_orphaned_tool_use_removed() {
    let messages = vec![
        TrackedMessage::user("run ls", "t1"),
        assistant_with_tool_use("call-orphan", "t1"),
        // No tool result for call-orphan
    ];
    let result = filter_orphaned_tool_uses(&messages);
    // User message kept, assistant message with orphaned tool_use removed
    assert_eq!(result.len(), 1);
    assert!(cocode_message::is_user_message(&result[0].inner));
}

#[test]
fn test_filter_mixed_resolved_and_orphaned() {
    let messages = vec![
        TrackedMessage::user("do things", "t1"),
        assistant_with_tool_use("call-ok", "t1"),
        tool_result("call-ok", "t1"),
        TrackedMessage::user("more", "t2"),
        assistant_with_tool_use("call-orphan", "t2"),
        // No result for call-orphan
    ];
    let result = filter_orphaned_tool_uses(&messages);
    // First 4 messages kept (user, assistant+tool_use, tool_result, user)
    // Last assistant with orphaned tool_use removed
    assert_eq!(result.len(), 4);
}

#[test]
fn test_filter_multiple_tool_calls_all_resolved() {
    let part1 = AssistantContentPart::ToolCall(ToolCallPart::new(
        "call-a",
        ToolName::Bash.as_str(),
        serde_json::json!({}),
    ));
    let part2 = AssistantContentPart::ToolCall(ToolCallPart::new(
        "call-b",
        ToolName::Read.as_str(),
        serde_json::json!({}),
    ));
    let assistant =
        cocode_message::create_assistant_message_with_content(vec![part1, part2], "t1", None);

    let messages = vec![
        TrackedMessage::user("do two things", "t1"),
        assistant,
        tool_result("call-a", "t1"),
        tool_result("call-b", "t1"),
    ];
    let result = filter_orphaned_tool_uses(&messages);
    assert_eq!(result.len(), 4);
}

#[test]
fn test_filter_multiple_tool_calls_partial_orphan() {
    // Assistant has two tool calls but only one has a result
    let part1 = AssistantContentPart::ToolCall(ToolCallPart::new(
        "call-a",
        ToolName::Bash.as_str(),
        serde_json::json!({}),
    ));
    let part2 = AssistantContentPart::ToolCall(ToolCallPart::new(
        "call-b",
        ToolName::Read.as_str(),
        serde_json::json!({}),
    ));
    let assistant =
        cocode_message::create_assistant_message_with_content(vec![part1, part2], "t1", None);

    let messages = vec![
        TrackedMessage::user("do two things", "t1"),
        assistant,
        tool_result("call-a", "t1"),
        // No result for call-b
    ];
    let result = filter_orphaned_tool_uses(&messages);
    // Assistant removed because not ALL tool_calls are resolved
    assert_eq!(result.len(), 2); // user + tool_result
}

#[test]
fn test_filter_distinguishes_different_tool_call_ids() {
    // Regression: ensures Tool message for call-X doesn't accidentally
    // resolve a different call-Y (the bug fixed in has_tool_result_for).
    let messages = vec![
        TrackedMessage::user("run two", "t1"),
        assistant_with_tool_use("call-X", "t1"),
        tool_result("call-Y", "t1"), // result for a DIFFERENT call
    ];
    let result = filter_orphaned_tool_uses(&messages);
    // call-X has no matching result, so the assistant message is removed
    assert_eq!(result.len(), 2); // user + tool_result(call-Y)
}
