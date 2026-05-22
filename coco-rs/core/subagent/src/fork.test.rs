use std::sync::Arc;
use uuid::Uuid;

use coco_llm_types::AssistantContentPart;
use coco_llm_types::LlmMessage;
use coco_llm_types::ToolCallPart;
use coco_llm_types::ToolContentPart;
use coco_llm_types::ToolResultContent;
use coco_llm_types::ToolResultPart;
use coco_types::messages::AssistantMessage;
use coco_types::messages::Message;
use coco_types::messages::ToolResultMessage;
use coco_types::messages::UserMessage;

use super::*;

fn assistant_with_tool_call(text: &str, tool_id: &str, tool_name: &str) -> Arc<Message> {
    Arc::new(Message::Assistant(AssistantMessage {
        message: LlmMessage::Assistant {
            content: vec![
                AssistantContentPart::text(text),
                AssistantContentPart::ToolCall(ToolCallPart::new(
                    tool_id,
                    tool_name,
                    serde_json::Value::Null,
                )),
            ],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        model: String::new(),
        stop_reason: None,
        usage: None,
        cost_usd: None,
        request_id: None,
        api_error: None,
    }))
}

fn tool_result(tool_use_id: &str, output_text: &str) -> Arc<Message> {
    Arc::new(Message::ToolResult(ToolResultMessage {
        uuid: Uuid::new_v4(),
        message: LlmMessage::Tool {
            content: vec![ToolContentPart::ToolResult(ToolResultPart::new(
                tool_use_id,
                "Bash",
                ToolResultContent::text(output_text),
            ))],
            provider_options: None,
        },
        tool_use_id: tool_use_id.to_string(),
        tool_id: "Bash".parse().unwrap(),
        is_error: false,
    }))
}

fn user_text(text: &str) -> Arc<Message> {
    Arc::new(Message::User(UserMessage {
        message: LlmMessage::user_text(text),
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: false,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: None,
        parent_tool_use_id: None,
    }))
}

fn extract_tool_result_text(arc: &Arc<Message>) -> &str {
    let Message::ToolResult(trm) = arc.as_ref() else {
        panic!("expected Message::ToolResult");
    };
    let LlmMessage::Tool { content, .. } = &trm.message else {
        panic!("expected LlmMessage::Tool");
    };
    let ToolContentPart::ToolResult(tr) = &content[0] else {
        panic!("expected ToolContentPart::ToolResult");
    };
    let ToolResultContent::Text { value, .. } = &tr.output else {
        panic!("expected ToolResultContent::Text");
    };
    value
}

#[test]
fn test_build_fork_context_replaces_tool_results() {
    let messages = vec![
        assistant_with_tool_call("Let me search", "tu_1", "Bash"),
        tool_result("tu_1", "file1.rs\nfile2.rs"),
        user_text("Do this task"),
    ];

    let ctx = build_fork_context(&messages, "Research the codebase");
    assert_eq!(ctx.messages.len(), 3);
    assert_eq!(ctx.directive, "Research the codebase");

    // The tool-result body must be replaced with FORK_PLACEHOLDER.
    assert_eq!(extract_tool_result_text(&ctx.messages[1]), FORK_PLACEHOLDER);

    // Assistant + plain user messages share Arc with the parent — no
    // allocation, identical pointer.
    assert!(Arc::ptr_eq(&ctx.messages[0], &messages[0]));
    assert!(Arc::ptr_eq(&ctx.messages[2], &messages[2]));
}

#[test]
fn test_build_fork_context_preserves_assistant() {
    let messages = vec![assistant_with_tool_call(
        "I found something",
        "tu_2",
        "Read",
    )];

    let ctx = build_fork_context(&messages, "Continue");
    assert_eq!(ctx.messages.len(), 1);
    assert!(Arc::ptr_eq(&ctx.messages[0], &messages[0]));
}

#[test]
fn test_build_fork_child_message_has_xml_tags() {
    let msg = build_fork_child_message("Find all TODO comments");
    assert!(msg.contains(&format!("<{FORK_BOILERPLATE_TAG}>")));
    assert!(msg.contains(&format!("</{FORK_BOILERPLATE_TAG}>")));
    assert!(msg.contains(FORK_DIRECTIVE_PREFIX));
    assert!(msg.contains("Find all TODO comments"));
    // The actual TS rule-body header — verified byte-for-byte against
    // `forkSubagent.ts:177`.
    assert!(msg.contains("RULES (non-negotiable):"));
    // Line that mentions "forked worker process" — also byte-for-byte.
    assert!(msg.contains("forked worker process"));
}

/// Byte-level TS alignment: `FORK_DIRECTIVE_PREFIX` must be exactly
/// `"Your directive: "` (trailing space, no newline) per
/// `constants/xml.ts:66`. Regression guard against the previous
/// `"Your task:\n"` bug.
#[test]
fn test_fork_directive_prefix_is_ts_aligned() {
    assert_eq!(FORK_DIRECTIVE_PREFIX, "Your directive: ");
}

/// The child message ends with `FORK_DIRECTIVE_PREFIX{directive}` and no
/// trailing newline — TS `forkSubagent.ts:197` template literal stops
/// at `${directive}`.
#[test]
fn test_build_fork_child_message_ends_with_directive() {
    let msg = build_fork_child_message("do the thing");
    assert!(
        msg.ends_with("Your directive: do the thing"),
        "message should end with prefix + directive, got tail: {:?}",
        &msg[msg.len().saturating_sub(60)..]
    );
}

/// Directive prefix appears after the closing tag, separated by exactly
/// one blank line — TS template literal has `</fork-boilerplate>\n\n{prefix}`.
#[test]
fn test_build_fork_child_message_blank_line_before_directive() {
    let msg = build_fork_child_message("x");
    let expected_seq = format!("</{FORK_BOILERPLATE_TAG}>\n\nYour directive: x");
    assert!(
        msg.ends_with(&expected_seq),
        "must have blank line between closing tag and directive prefix"
    );
}

#[test]
fn test_build_worktree_notice_ts_byte_faithful() {
    // TS `forkSubagent.ts:205-210`. Lock the full text — the brevity
    // of the old assertion let a 110-char rewrite slip through.
    let got = build_worktree_notice("/parent/dir", "/worktree/dir");
    let expected = "You've inherited the conversation context above from a parent agent working in /parent/dir. You are operating in an isolated git worktree at /worktree/dir \u{2014} same repository, same relative file structure, separate working copy. Paths in the inherited context refer to the parent's working directory; translate them to your worktree root. Re-read files before editing if the parent may have modified them since they appear in the context. Your changes stay in this worktree and will not affect the parent's files.";
    assert_eq!(got, expected);
}

#[test]
fn test_is_in_fork_child_detects_tag() {
    let messages = vec![user_text(&format!(
        "<{FORK_BOILERPLATE_TAG}>\nrules\n</{FORK_BOILERPLATE_TAG}>"
    ))];
    assert!(is_in_fork_child(&messages));
}

#[test]
fn test_is_in_fork_child_no_tag() {
    let messages = vec![user_text("normal message")];
    assert!(!is_in_fork_child(&messages));
}

#[test]
fn test_is_in_fork_child_assistant_messages_ignored() {
    let messages = vec![Arc::new(Message::Assistant(AssistantMessage {
        message: LlmMessage::Assistant {
            content: vec![AssistantContentPart::text(format!(
                "<{FORK_BOILERPLATE_TAG}>rules</{FORK_BOILERPLATE_TAG}>"
            ))],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        model: String::new(),
        stop_reason: None,
        usage: None,
        cost_usd: None,
        request_id: None,
        api_error: None,
    }))];
    assert!(!is_in_fork_child(&messages));
}

#[test]
fn test_is_fork_allowed_guards() {
    // Fork disabled by default (env var not set)
    assert!(!is_fork_allowed(0, None, &[]));
    assert!(!is_fork_allowed(1, None, &[]));
    assert!(!is_fork_allowed(0, Some("Explore"), &[]));
}

#[test]
fn test_build_fork_context_empty_messages() {
    let ctx = build_fork_context(&[], "directive");
    assert!(ctx.messages.is_empty());
}

#[test]
fn test_build_fork_context_plain_user_passes_through() {
    let messages = vec![user_text("plain text")];
    let ctx = build_fork_context(&messages, "test");
    assert_eq!(ctx.messages.len(), 1);
    // Plain user message shares Arc with input — no allocation, no rewrite.
    assert!(Arc::ptr_eq(&ctx.messages[0], &messages[0]));
}
