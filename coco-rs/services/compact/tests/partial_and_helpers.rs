//! Integration tests for `partial_compact_conversation` and the
//! token-gap-aware PTL retry path, plus the reactive `peel_head` helper.

use coco_compact::compact::partial_compact_conversation;
use coco_compact::compact::truncate_head_for_ptl_retry;
use coco_compact::reactive::peel_head_for_ptl_retry;
use coco_test_harness::compact as mock;
use coco_test_harness::conversation;
use coco_test_harness::messages as msg;
use coco_types::CompactTrigger;
use coco_types::Message;
use coco_types::PartialCompactDirection;
use coco_types::SystemMessage;

const SUMMARY: &str = "<analysis>Reviewed.</analysis><summary>Summary of recent work.</summary>";

#[tokio::test]
async fn partial_newest_summarizes_tail_keeps_prefix() {
    // 6 turns; pivot at index 4 → summarize messages[4..], keep messages[..4].
    let messages = conversation::simple(6);
    let result = partial_compact_conversation(
        &messages,
        4,
        PartialCompactDirection::Newest,
        None,
        None,
        mock::mock_summarize_ok(SUMMARY),
        None,
    )
    .await
    .expect("partial compact should succeed");

    mock::assert_boundary_valid(&result);
    mock::assert_summary_valid(&result);
    assert_eq!(result.trigger, CompactTrigger::Manual);

    // For Newest direction, prefix is kept.
    assert!(!result.messages_to_keep.is_empty());

    // Boundary must record the preserved segment with the boundary uuid as anchor.
    let Message::System(SystemMessage::CompactBoundary(b)) = &result.boundary_marker else {
        panic!("expected CompactBoundary");
    };
    let seg = b
        .preserved_segment
        .as_ref()
        .expect("preserved_segment should be set");
    assert_eq!(seg.anchor_uuid, b.uuid, "Newest anchor must be boundary");
}

#[tokio::test]
async fn partial_oldest_summarizes_prefix_keeps_tail() {
    let messages = conversation::simple(6);
    let result = partial_compact_conversation(
        &messages,
        2,
        PartialCompactDirection::Oldest,
        Some("focus on tests"),
        None,
        mock::mock_summarize_ok(SUMMARY),
        None,
    )
    .await
    .expect("partial compact should succeed");

    let Message::System(SystemMessage::CompactBoundary(b)) = &result.boundary_marker else {
        panic!("expected CompactBoundary");
    };
    // user_feedback was supplied — must propagate to user_context.
    assert_eq!(b.user_context.as_deref(), Some("focus on tests"));

    // Anchor for Oldest = last summary uuid (suffix-preserving chain).
    let summary_uuid = match &result.summary_messages[0] {
        Message::User(u) => u.uuid,
        _ => panic!("summary[0] must be User"),
    };
    let seg = b
        .preserved_segment
        .as_ref()
        .expect("preserved_segment should be set");
    assert_eq!(
        seg.anchor_uuid, summary_uuid,
        "Oldest anchor = last summary"
    );
}

#[tokio::test]
async fn partial_empty_summarize_errors() {
    let messages = conversation::simple(2);
    // Newest at end → nothing to summarize.
    let res = partial_compact_conversation(
        &messages,
        messages.len(),
        PartialCompactDirection::Newest,
        None,
        None,
        mock::mock_summarize_ok(SUMMARY),
        None,
    )
    .await;
    assert!(res.is_err(), "should error when summarize range is empty");
}

#[test]
fn truncate_head_uses_token_gap_when_provided() {
    let messages = conversation::simple(4);
    // Provide a tiny gap → should drop just one group.
    let truncated = truncate_head_for_ptl_retry(&messages, Some(1), 0.2)
        .expect("with multiple groups, returns a survivor list");
    assert!(
        truncated.len() < messages.len(),
        "should drop at least one group"
    );
}

#[test]
fn truncate_head_strips_stale_marker_before_grouping() {
    use coco_types::LlmMessage;
    use coco_types::UserMessage;
    use uuid::Uuid;

    // Marker at the head + 4 turns → grouping must skip the marker first.
    let mut messages = vec![Message::User(UserMessage {
        message: LlmMessage::user_text("[earlier conversation truncated for compaction retry]"),
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: true,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: None,
        parent_tool_use_id: None,
    })];
    messages.extend(conversation::simple(4));

    // Without the strip, the function would see only the marker as group 0
    // and might fail. With the strip, it operates on the real conversation.
    let result = truncate_head_for_ptl_retry(&messages, None, 0.5);
    assert!(
        result.is_some(),
        "PTL retry should succeed when stripping stale marker"
    );
}

#[test]
fn truncate_head_returns_none_with_one_group() {
    // Single user message, no assistant ⇒ one group ⇒ nothing to drop.
    let messages = vec![msg::user("only one message")];
    assert!(truncate_head_for_ptl_retry(&messages, None, 0.5).is_none());
}

#[test]
fn peel_head_drops_oldest_groups() {
    // Build a multi-round conversation by alternating user/assistant.
    let messages = conversation::simple(6);
    let total_tokens = coco_compact::estimate_tokens(&messages);
    let target = total_tokens / 2;
    let peeled = peel_head_for_ptl_retry(&messages, target).expect("should peel some groups");
    assert!(
        peeled.len() < messages.len(),
        "must drop at least one group"
    );
    // After peeling, total tokens are lower.
    assert!(coco_compact::estimate_tokens(&peeled) <= total_tokens);
}

#[test]
fn peel_head_returns_none_for_single_group() {
    let messages = vec![msg::user("hi")];
    assert!(peel_head_for_ptl_retry(&messages, 1).is_none());
}

#[test]
fn build_post_compact_messages_has_canonical_order() {
    let mut result = mock::dummy_compact_result();
    result.summary_messages.push(msg::user("summary"));
    result.messages_to_keep.push(msg::user("kept"));
    result.hook_results.push(msg::user("hook"));

    let assembled = coco_compact::build_post_compact_messages(&result);
    assert_eq!(assembled.len(), 4); // boundary + summary + kept + hook
    matches!(
        assembled[0],
        Message::System(SystemMessage::CompactBoundary(_))
    );
}

#[test]
fn merge_hook_instructions_combines_both() {
    let merged = coco_compact::merge_hook_instructions(Some("user text"), Some("hook addition"));
    assert_eq!(merged.as_deref(), Some("user text\n\nhook addition"));

    assert_eq!(
        coco_compact::merge_hook_instructions(Some("only user"), None).as_deref(),
        Some("only user")
    );
    assert_eq!(
        coco_compact::merge_hook_instructions(None, Some("only hook")).as_deref(),
        Some("only hook")
    );
    assert_eq!(coco_compact::merge_hook_instructions(None, None), None);
    // Empty strings collapse to None.
    assert_eq!(
        coco_compact::merge_hook_instructions(Some("   "), Some("   ")),
        None
    );
}

#[test]
fn extract_discovered_tool_names_picks_up_toolsearch_input() {
    use coco_types::AssistantContent;
    use coco_types::AssistantMessage;
    use coco_types::LlmMessage;
    use serde_json::json;
    use uuid::Uuid;

    let messages = vec![Message::Assistant(AssistantMessage {
        message: LlmMessage::Assistant {
            content: vec![AssistantContent::ToolCall(
                vercel_ai_provider::ToolCallPart {
                    tool_call_id: "call_ts".into(),
                    tool_name: "ToolSearch".into(),
                    input: json!({"tools": ["Bash", "Edit"]}),
                    provider_executed: None,
                    provider_metadata: None,
                },
            )],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        model: "test".into(),
        stop_reason: None,
        usage: None,
        cost_usd: None,
        request_id: None,
        api_error: None,
    })];

    let names = coco_compact::extract_discovered_tool_names(&messages);
    assert_eq!(names.len(), 2);
    assert!(names.contains("Bash"));
    assert!(names.contains("Edit"));
}
