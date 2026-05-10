use crate::*;
use uuid::Uuid;

use super::ensure_user_first;
use super::merge_consecutive_assistant_messages;
use super::merge_consecutive_user_messages;
use super::normalize_messages_for_api;
use super::strip_images_from_messages;
use super::strip_signature_blocks;
use super::to_llm_prompt;

fn user_msg(text: &str) -> Message {
    Message::User(UserMessage {
        message: LlmMessage::user_text(text),
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: false,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: None,
        parent_tool_use_id: None,
    })
}

fn virtual_msg(text: &str) -> Message {
    Message::User(UserMessage {
        message: LlmMessage::user_text(text),
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: false,
        is_virtual: true,
        is_compact_summary: false,
        permission_mode: None,
        origin: None,
        parent_tool_use_id: None,
    })
}

fn assistant_msg(text: &str) -> Message {
    Message::Assistant(AssistantMessage {
        message: LlmMessage::Assistant {
            content: vec![AssistantContent::Text(TextContent {
                text: text.into(),
                provider_metadata: None,
            })],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        model: "test".into(),
        stop_reason: Some(StopReason::EndTurn),
        usage: None,
        cost_usd: None,
        request_id: None,
        api_error: None,
    })
}

fn tombstone_msg() -> Message {
    Message::Tombstone(TombstoneMessage {
        uuid: Uuid::new_v4(),
        original_kind: MessageKind::User,
    })
}

#[test]
fn test_filters_virtual_messages() {
    let msgs = vec![user_msg("hello"), virtual_msg("ghost"), assistant_msg("hi")];
    let result = normalize_messages_for_api(&msgs);
    assert_eq!(result.len(), 2); // virtual filtered out
}

#[test]
fn test_filters_tombstones() {
    let msgs = vec![user_msg("hello"), tombstone_msg(), assistant_msg("hi")];
    let result = normalize_messages_for_api(&msgs);
    assert_eq!(result.len(), 2); // tombstone filtered out
}

#[test]
fn test_merges_consecutive_user_messages() {
    let msgs = vec![user_msg("hello"), user_msg("world"), assistant_msg("hi")];
    let result = normalize_messages_for_api(&msgs);
    // Two user messages merged into one, plus assistant = 2
    assert_eq!(result.len(), 2);
}

#[test]
fn test_ensures_starts_with_user() {
    let msgs = vec![assistant_msg("hi")];
    let result = normalize_messages_for_api(&msgs);
    assert!(matches!(&result[0], LlmMessage::User { .. }));
}

#[test]
fn test_empty_input() {
    let result = normalize_messages_for_api(&[]);
    assert!(result.is_empty());
}

#[test]
fn test_progress_messages_filtered() {
    let msgs = vec![
        user_msg("hello"),
        Message::Progress(ProgressMessage {
            tool_use_id: "test".into(),
            data: serde_json::Value::Null,
            parent_message_uuid: None,
        }),
        assistant_msg("hi"),
    ];
    let result = normalize_messages_for_api(&msgs);
    assert_eq!(result.len(), 2); // progress filtered
}

// === merge_consecutive_user_messages ===

#[test]
fn test_merge_consecutive_user_messages_basic() {
    let mut msgs = vec![user_msg("hello"), user_msg("world"), assistant_msg("hi")];
    merge_consecutive_user_messages(&mut msgs);
    assert_eq!(msgs.len(), 2);
    // First message should have merged content.
    if let Message::User(u) = &msgs[0] {
        if let LlmMessage::User { content, .. } = &u.message {
            assert_eq!(content.len(), 2); // two text parts merged
        } else {
            panic!("expected User LlmMessage");
        }
    } else {
        panic!("expected User message");
    }
}

#[test]
fn test_merge_consecutive_user_messages_empty() {
    let mut msgs: Vec<Message> = vec![];
    merge_consecutive_user_messages(&mut msgs);
    assert!(msgs.is_empty());
}

#[test]
fn test_merge_consecutive_user_messages_single() {
    let mut msgs = vec![user_msg("only")];
    merge_consecutive_user_messages(&mut msgs);
    assert_eq!(msgs.len(), 1);
}

#[test]
fn test_merge_consecutive_user_messages_no_merge() {
    let mut msgs = vec![user_msg("a"), assistant_msg("b"), user_msg("c")];
    merge_consecutive_user_messages(&mut msgs);
    assert_eq!(msgs.len(), 3);
}

#[test]
fn test_merge_consecutive_user_messages_three() {
    let mut msgs = vec![user_msg("a"), user_msg("b"), user_msg("c")];
    merge_consecutive_user_messages(&mut msgs);
    assert_eq!(msgs.len(), 1);
    if let Message::User(u) = &msgs[0] {
        if let LlmMessage::User { content, .. } = &u.message {
            assert_eq!(content.len(), 3);
        } else {
            panic!("expected User LlmMessage");
        }
    } else {
        panic!("expected User message");
    }
}

// === merge_consecutive_assistant_messages ===

#[test]
fn test_merge_consecutive_assistant_messages_basic() {
    let mut msgs = vec![user_msg("hi"), assistant_msg("a"), assistant_msg("b")];
    merge_consecutive_assistant_messages(&mut msgs);
    assert_eq!(msgs.len(), 2);
    if let Message::Assistant(a) = &msgs[1] {
        if let LlmMessage::Assistant { content, .. } = &a.message {
            assert_eq!(content.len(), 2);
        } else {
            panic!("expected Assistant LlmMessage");
        }
    } else {
        panic!("expected Assistant message");
    }
}

#[test]
fn test_merge_consecutive_assistant_messages_empty() {
    let mut msgs: Vec<Message> = vec![];
    merge_consecutive_assistant_messages(&mut msgs);
    assert!(msgs.is_empty());
}

#[test]
fn test_merge_consecutive_assistant_messages_no_merge() {
    let mut msgs = vec![assistant_msg("a"), user_msg("b"), assistant_msg("c")];
    merge_consecutive_assistant_messages(&mut msgs);
    assert_eq!(msgs.len(), 3);
}

// === strip_images_from_messages ===

fn user_msg_with_image() -> Message {
    use crate::UserContent;
    use coco_inference::FilePart;
    use coco_inference::TextPart;

    Message::User(UserMessage {
        message: LlmMessage::User {
            content: vec![
                UserContent::Text(TextPart::new("caption")),
                UserContent::File(FilePart::from_base64("abc123", "image/png")),
            ],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: false,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: None,
        parent_tool_use_id: None,
    })
}

fn user_msg_image_only() -> Message {
    use crate::UserContent;
    use coco_inference::FilePart;

    Message::User(UserMessage {
        message: LlmMessage::User {
            content: vec![UserContent::File(FilePart::from_base64(
                "abc123",
                "image/jpeg",
            ))],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: false,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: None,
        parent_tool_use_id: None,
    })
}

#[test]
fn test_strip_images_keeps_text() {
    let mut msgs = vec![user_msg_with_image()];
    strip_images_from_messages(&mut msgs);
    assert_eq!(msgs.len(), 1);
    if let Message::User(u) = &msgs[0] {
        if let LlmMessage::User { content, .. } = &u.message {
            assert_eq!(content.len(), 1);
            assert!(matches!(content[0], crate::UserContent::Text(_)));
        } else {
            panic!("expected User LlmMessage");
        }
    } else {
        panic!("expected User message");
    }
}

#[test]
fn test_strip_images_removes_empty_messages() {
    let mut msgs = vec![user_msg("keep"), user_msg_image_only()];
    strip_images_from_messages(&mut msgs);
    assert_eq!(msgs.len(), 1);
}

#[test]
fn test_strip_images_empty_input() {
    let mut msgs: Vec<Message> = vec![];
    strip_images_from_messages(&mut msgs);
    assert!(msgs.is_empty());
}

#[test]
fn test_strip_images_preserves_assistant() {
    let mut msgs = vec![user_msg_with_image(), assistant_msg("hi")];
    strip_images_from_messages(&mut msgs);
    assert_eq!(msgs.len(), 2);
}

// === strip_signature_blocks ===

fn user_msg_with_sig(text: &str) -> Message {
    Message::User(UserMessage {
        message: LlmMessage::user_text(text),
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: false,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: None,
        parent_tool_use_id: None,
    })
}

#[test]
fn test_strip_signature_basic() {
    let mut msgs = vec![user_msg_with_sig("Hello\n-- \nJohn Doe")];
    strip_signature_blocks(&mut msgs);
    if let Message::User(u) = &msgs[0] {
        if let LlmMessage::User { content, .. } = &u.message {
            if let crate::UserContent::Text(t) = &content[0] {
                assert_eq!(t.text, "Hello");
            } else {
                panic!("expected text");
            }
        } else {
            panic!("expected user llm msg");
        }
    } else {
        panic!("expected user msg");
    }
}

#[test]
fn test_strip_signature_no_sig() {
    let mut msgs = vec![user_msg_with_sig("No signature here")];
    strip_signature_blocks(&mut msgs);
    if let Message::User(u) = &msgs[0] {
        if let LlmMessage::User { content, .. } = &u.message {
            if let crate::UserContent::Text(t) = &content[0] {
                assert_eq!(t.text, "No signature here");
            } else {
                panic!("expected text");
            }
        } else {
            panic!("expected user llm msg");
        }
    } else {
        panic!("expected user msg");
    }
}

#[test]
fn test_strip_signature_empty() {
    let mut msgs: Vec<Message> = vec![];
    strip_signature_blocks(&mut msgs);
    assert!(msgs.is_empty());
}

// === ensure_user_first ===

#[test]
fn test_ensure_user_first_already_user() {
    let mut msgs = vec![user_msg("hello"), assistant_msg("hi")];
    ensure_user_first(&mut msgs);
    assert_eq!(msgs.len(), 2);
}

#[test]
fn test_ensure_user_first_prepends() {
    let mut msgs = vec![assistant_msg("hi")];
    ensure_user_first(&mut msgs);
    assert_eq!(msgs.len(), 2);
    assert!(matches!(msgs[0], Message::User(_)));
}

#[test]
fn test_ensure_user_first_empty() {
    let mut msgs: Vec<Message> = vec![];
    ensure_user_first(&mut msgs);
    assert!(msgs.is_empty());
}

// === to_llm_prompt ===

#[test]
fn test_to_llm_prompt_basic() {
    let msgs = vec![user_msg("hello"), assistant_msg("hi")];
    let prompt = to_llm_prompt(&msgs);
    assert_eq!(prompt.len(), 2);
    assert!(matches!(prompt[0], LlmMessage::User { .. }));
    assert!(matches!(prompt[1], LlmMessage::Assistant { .. }));
}

#[test]
fn test_to_llm_prompt_skips_system_and_progress() {
    let msgs = vec![
        user_msg("hello"),
        Message::Progress(ProgressMessage {
            tool_use_id: "test".into(),
            data: serde_json::Value::Null,
            parent_message_uuid: None,
        }),
        tombstone_msg(),
        assistant_msg("hi"),
    ];
    let prompt = to_llm_prompt(&msgs);
    // Progress, Tombstone are skipped.
    assert_eq!(prompt.len(), 2);
}

#[test]
fn test_to_llm_prompt_empty() {
    let prompt = to_llm_prompt(&[]);
    assert!(prompt.is_empty());
}

// ─────────────────────────────────────────────────────────────────────
// TS-parity regression tests for Round 10 deep-review fixes.
// ─────────────────────────────────────────────────────────────────────

fn assistant_msg_with_request_id(text: &str, request_id: &str) -> Message {
    Message::Assistant(AssistantMessage {
        message: LlmMessage::Assistant {
            content: vec![AssistantContent::Text(TextContent {
                text: text.into(),
                provider_metadata: None,
            })],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        model: "test".into(),
        stop_reason: Some(StopReason::EndTurn),
        usage: None,
        cost_usd: None,
        request_id: Some(request_id.into()),
        api_error: None,
    })
}

#[test]
fn merge_assistants_by_request_id_keeps_distinct_ids_separate() {
    // TS messages.ts:2257-2261 — different message.id stays separate.
    let msgs = vec![
        user_msg("hi"),
        assistant_msg_with_request_id("first", "req-A"),
        assistant_msg_with_request_id("second", "req-B"),
    ];
    let prompt = normalize_messages_for_api(&msgs);
    // 1 user + 2 assistants (NOT merged, since request_ids differ).
    let asst_count = prompt
        .iter()
        .filter(|m| matches!(m, LlmMessage::Assistant { .. }))
        .count();
    assert_eq!(
        asst_count, 2,
        "different request_ids must NOT merge — TS only merges matching message.id"
    );
}

#[test]
fn merge_assistants_by_request_id_merges_matching_ids() {
    let msgs = vec![
        user_msg("hi"),
        assistant_msg_with_request_id("first", "req-A"),
        assistant_msg_with_request_id("second", "req-A"),
    ];
    let prompt = normalize_messages_for_api(&msgs);
    let asst_count = prompt
        .iter()
        .filter(|m| matches!(m, LlmMessage::Assistant { .. }))
        .count();
    assert_eq!(
        asst_count, 1,
        "matching request_ids merge — TS streaming chunks"
    );
}

#[test]
fn merge_assistants_with_no_request_id_stays_separate() {
    // TS strict equality: undefined !== undefined in this comparison.
    // Synthetic / test messages with no request_id never merge.
    let msgs = vec![
        user_msg("hi"),
        assistant_msg("first"),  // request_id = None
        assistant_msg("second"), // request_id = None
    ];
    let prompt = normalize_messages_for_api(&msgs);
    let asst_count = prompt
        .iter()
        .filter(|m| matches!(m, LlmMessage::Assistant { .. }))
        .count();
    assert_eq!(
        asst_count, 2,
        "request_id=None never matches itself — keeps assistants separate"
    );
}

#[test]
fn smoosh_folds_into_is_error_tool_result() {
    use coco_inference::ToolContentPart;
    use coco_inference::ToolResultContent;
    use coco_inference::ToolResultPart;

    // Construct a Tool LlmMessage with an is_error=true tool_result whose
    // output is a Content array. TS smooshIntoToolResult filters incoming
    // blocks to text-only and proceeds — Rust must do the same.
    let tool_msg = LlmMessage::Tool {
        content: vec![ToolContentPart::ToolResult(ToolResultPart {
            tool_call_id: "tc1".into(),
            tool_name: "Bash".into(),
            output: ToolResultContent::Text {
                value: "error output".into(),
                provider_options: None,
            },
            is_error: true,
            provider_metadata: None,
        })],
        provider_options: None,
    };
    let user_sr = LlmMessage::User {
        content: vec![coco_inference::UserContentPart::text(
            "<system-reminder>\nctx\n</system-reminder>",
        )],
        provider_options: None,
    };

    let mut msgs = vec![tool_msg, user_sr];
    super::smoosh_system_reminder_into_tool_result(&mut msgs);

    assert_eq!(msgs.len(), 1, "SR-User must fold into is_error tool_result");
    let LlmMessage::Tool { content, .. } = &msgs[0] else {
        panic!("expected Tool LlmMessage");
    };
    let ToolContentPart::ToolResult(rp) = &content[0] else {
        panic!("expected ToolResult part");
    };
    let ToolResultContent::Text { value, .. } = &rp.output else {
        panic!("expected Text output");
    };
    assert!(
        value.contains("<system-reminder>"),
        "SR text must be folded into is_error tool_result (TS-parity fix)"
    );
}

#[test]
fn sanitize_strips_non_text_from_is_error_tool_result() {
    use coco_inference::ToolContentPart;
    use coco_inference::ToolResultContent;
    use coco_inference::ToolResultContentPart;
    use coco_inference::ToolResultPart;

    // is_error=true with a mixed Content array (text + image). The image
    // part must be stripped; surviving texts join with \n\n.
    let mut msgs = vec![LlmMessage::Tool {
        content: vec![ToolContentPart::ToolResult(ToolResultPart {
            tool_call_id: "tc1".into(),
            tool_name: "Bash".into(),
            output: ToolResultContent::Content {
                value: vec![
                    ToolResultContentPart::Text {
                        text: "stderr line 1".into(),
                        provider_options: None,
                    },
                    ToolResultContentPart::FileData {
                        data: "iVBORw0KGgo=".into(),
                        media_type: "image/png".into(),
                        filename: None,
                        provider_options: None,
                    },
                    ToolResultContentPart::Text {
                        text: "stderr line 2".into(),
                        provider_options: None,
                    },
                ],
                provider_options: None,
            },
            is_error: true,
            provider_metadata: None,
        })],
        provider_options: None,
    }];

    super::sanitize_error_tool_result_in_llm_messages(&mut msgs);

    let LlmMessage::Tool { content, .. } = &msgs[0] else {
        panic!("expected Tool LlmMessage");
    };
    let ToolContentPart::ToolResult(rp) = &content[0] else {
        panic!("expected ToolResult");
    };
    let ToolResultContent::Content { value, .. } = &rp.output else {
        panic!("expected Content variant");
    };
    assert_eq!(value.len(), 1, "joined into one Text part");
    let ToolResultContentPart::Text { text, .. } = &value[0] else {
        panic!("expected Text after sanitize");
    };
    assert_eq!(text, "stderr line 1\n\nstderr line 2");
}

/// TS-parity forward synthesis (`utils/messages.ts:5301-5326`):
/// when an assistant tool_use has no matching tool_result anywhere
/// in the transcript, normalize_messages_for_api must inject an
/// `is_error: true` placeholder so the next provider call doesn't
/// hit `unexpected tool_use_id`.
#[test]
fn normalize_synthesizes_missing_tool_result() {
    use coco_inference::ToolCallPart;
    use coco_inference::ToolContentPart;
    use coco_inference::ToolResultContent;
    use coco_types::ToolId;
    use coco_types::ToolName;

    // Assistant emits tc1 + tc2; only tc1 has a result.
    let assistant = Message::Assistant(AssistantMessage {
        message: LlmMessage::Assistant {
            content: vec![
                AssistantContent::Text(coco_inference::TextPart {
                    text: "calling".into(),
                    provider_metadata: None,
                }),
                AssistantContent::ToolCall(ToolCallPart::new(
                    "tc1",
                    "Bash",
                    serde_json::json!({"command": "echo a"}),
                )),
                AssistantContent::ToolCall(ToolCallPart::new(
                    "tc2",
                    "Bash",
                    serde_json::json!({"command": "echo b"}),
                )),
            ],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        model: "test".into(),
        stop_reason: Some(StopReason::EndTurn),
        usage: None,
        cost_usd: None,
        request_id: None,
        api_error: None,
    });
    let tool_result_tc1 = Message::ToolResult(ToolResultMessage {
        uuid: Uuid::new_v4(),
        message: LlmMessage::Tool {
            content: vec![ToolContentPart::ToolResult(
                coco_inference::ToolResultPart {
                    tool_call_id: "tc1".into(),
                    tool_name: "Bash".into(),
                    output: ToolResultContent::text("a"),
                    is_error: false,
                    provider_metadata: None,
                },
            )],
            provider_options: None,
        },
        tool_use_id: "tc1".into(),
        tool_id: ToolId::Builtin(ToolName::Bash),
        is_error: false,
    });

    let result = normalize_messages_for_api(&[user_msg("go"), assistant, tool_result_tc1]);

    // Find the Tool message — it must now carry BOTH tc1 (real) and
    // tc2 (synthesized is_error). Order: real first (it predates the
    // synthesizer), synthetic appended.
    let tool_idx = result
        .iter()
        .position(|m| matches!(m, LlmMessage::Tool { .. }))
        .expect("Tool message must exist");
    let LlmMessage::Tool { content, .. } = &result[tool_idx] else {
        unreachable!()
    };
    let ids: Vec<(String, bool)> = content
        .iter()
        .filter_map(|p| match p {
            ToolContentPart::ToolResult(r) => Some((r.tool_call_id.clone(), r.is_error)),
            _ => None,
        })
        .collect();
    assert!(
        ids.iter().any(|(id, e)| id == "tc1" && !*e),
        "tc1 real result must survive: got {ids:?}"
    );
    assert!(
        ids.iter().any(|(id, e)| id == "tc2" && *e),
        "tc2 synthetic is_error result must be injected: got {ids:?}"
    );
}

/// Synthesis must be idempotent — running it twice on the same Vec
/// must not produce duplicate tool_results. Calls the private
/// `synthesize_missing_tool_results` directly (twice) so this test
/// genuinely exercises the second-pass path, not just observes a
/// single normalize-call output.
#[test]
fn normalize_synthesis_is_idempotent() {
    use coco_inference::ToolCallPart;
    use coco_inference::ToolContentPart;

    // Build a Vec<LlmMessage> with one assistant tool_use and no
    // matching tool_result.
    let mut msgs = vec![
        LlmMessage::user_text("go"),
        LlmMessage::Assistant {
            content: vec![AssistantContent::ToolCall(ToolCallPart::new(
                "orphan",
                "Bash",
                serde_json::json!({}),
            ))],
            provider_options: None,
        },
    ];

    super::synthesize_missing_tool_results(&mut msgs);
    let count_after_first = count_tool_results_for(&msgs, "orphan");
    assert_eq!(count_after_first, 1, "first pass must inject the synthetic");

    // Second call on the SAME mutated Vec — the synthetic from pass 1
    // is now in `resolved`, so pass 2 must be a no-op.
    super::synthesize_missing_tool_results(&mut msgs);
    let count_after_second = count_tool_results_for(&msgs, "orphan");
    assert_eq!(
        count_after_second, 1,
        "second pass must NOT duplicate the synthetic; got {count_after_second} parts"
    );

    fn count_tool_results_for(msgs: &[LlmMessage], id: &str) -> usize {
        msgs.iter()
            .map(|m| match m {
                LlmMessage::Tool { content, .. } => content
                    .iter()
                    .filter(|p| matches!(p, ToolContentPart::ToolResult(r) if r.tool_call_id == id))
                    .count(),
                _ => 0,
            })
            .sum()
    }
}

/// Two assistants with orphans, separated by a Tool message that
/// resolves a different tool_use. Each assistant's orphan must be
/// synthesized independently — verifies the audit's edge case A2.
#[test]
fn normalize_synthesizes_for_multiple_assistants() {
    use coco_inference::ToolCallPart;
    use coco_inference::ToolContentPart;
    use coco_types::ToolId;
    use coco_types::ToolName;

    let asst_a = Message::Assistant(AssistantMessage {
        message: LlmMessage::Assistant {
            content: vec![AssistantContent::ToolCall(ToolCallPart::new(
                "tcA",
                "Bash",
                serde_json::json!({}),
            ))],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        model: "test".into(),
        stop_reason: Some(StopReason::EndTurn),
        usage: None,
        cost_usd: None,
        request_id: None,
        api_error: None,
    });
    // Tool that resolves an unrelated id (decoy — must NOT be treated
    // as covering tcA or tcB).
    let unrelated = Message::ToolResult(ToolResultMessage {
        uuid: Uuid::new_v4(),
        message: LlmMessage::Tool {
            content: vec![ToolContentPart::ToolResult(
                coco_inference::ToolResultPart {
                    tool_call_id: "unrelated".into(),
                    tool_name: "Bash".into(),
                    output: coco_inference::ToolResultContent::text("ok"),
                    is_error: false,
                    provider_metadata: None,
                },
            )],
            provider_options: None,
        },
        tool_use_id: "unrelated".into(),
        tool_id: ToolId::Builtin(ToolName::Bash),
        is_error: false,
    });
    let asst_b = Message::Assistant(AssistantMessage {
        message: LlmMessage::Assistant {
            content: vec![AssistantContent::ToolCall(ToolCallPart::new(
                "tcB",
                "Bash",
                serde_json::json!({}),
            ))],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        model: "test".into(),
        stop_reason: Some(StopReason::EndTurn),
        usage: None,
        cost_usd: None,
        request_id: None,
        api_error: None,
    });

    let result = normalize_messages_for_api(&[user_msg("go"), asst_a, unrelated, asst_b]);

    let collect_ids = |id: &str| {
        result
            .iter()
            .flat_map(|m| match m {
                LlmMessage::Tool { content, .. } => content
                    .iter()
                    .filter_map(|p| match p {
                        ToolContentPart::ToolResult(r) if r.tool_call_id == id => Some(r.is_error),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .collect::<Vec<_>>()
    };

    let tca = collect_ids("tcA");
    let tcb = collect_ids("tcB");
    assert_eq!(
        tca,
        vec![true],
        "tcA must have exactly one synthetic is_error result"
    );
    assert_eq!(
        tcb,
        vec![true],
        "tcB must have exactly one synthetic is_error result"
    );
}
