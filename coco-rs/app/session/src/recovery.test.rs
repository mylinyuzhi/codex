use super::*;
use coco_messages::Message;
use serde_json::json;

#[test]
fn test_can_resume_nonexistent() {
    assert!(!can_resume_session(Path::new("/nonexistent/path.jsonl")));
}

#[test]
fn test_fork_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("source.jsonl");
    let dst = dir.path().join("dest.jsonl");
    std::fs::write(&src, "{\"test\": true}\n").unwrap();
    fork_conversation(&src, &dst).unwrap();
    assert!(dst.exists());
    assert_eq!(
        std::fs::read_to_string(&dst).unwrap(),
        std::fs::read_to_string(&src).unwrap()
    );
}

// ---------------------------------------------------------------------------
// load_conversation_for_resume — the actual resume entry point.
// Each test writes a synthetic JSONL transcript and walks the function.
// ---------------------------------------------------------------------------

/// Build a JSONL line for a `user` transcript entry with plain-text content.
///
/// `parent` matches TS's `parentUuid` linkage — every user message
/// after the first should chain to the prior assistant so the resume
/// walker reconstructs the full chain via DAG traversal. Uses the
/// camelCase wire shape (`sessionId`, `parentUuid`, `isSidechain`)
/// the on-disk JSONL actually carries — TS-compat by design.
fn user_line_with_parent(uuid: &str, parent: Option<&str>, text: &str) -> String {
    let mut entry = json!({
        "type": "user",
        "uuid": uuid,
        "sessionId": "s1",
        "cwd": "/tmp/p",
        "timestamp": "2025-01-15T10:00:00Z",
        "isSidechain": false,
        "message": { "role": "user", "content": [{"type": "text", "text": text}] },
    });
    if let Some(p) = parent {
        entry["parentUuid"] = json!(p);
    }
    serde_json::to_string(&entry).unwrap()
}

/// Convenience for the common "first user message in a session" case.
fn user_line(uuid: &str, text: &str) -> String {
    user_line_with_parent(uuid, None, text)
}

/// Build a JSONL line for an `assistant` entry, optionally with a usage block
/// so the loader's token aggregation has something to sum.
fn assistant_line(
    uuid: &str,
    parent: &str,
    text: &str,
    model: &str,
    usage: Option<(i64, i64)>,
) -> String {
    let mut entry = json!({
        "type": "assistant",
        "uuid": uuid,
        "parentUuid": parent,
        "sessionId": "s1",
        "cwd": "/tmp/p",
        "timestamp": "2025-01-15T10:00:01Z",
        "isSidechain": false,
        "message": {
            "role": "assistant",
            "content": [{"type": "text", "text": text}],
        },
        "model": model,
    });
    if let Some((input, output)) = usage {
        entry["usage"] = json!({
            "inputTokens": input,
            "outputTokens": output,
        });
    }
    serde_json::to_string(&entry).unwrap()
}

#[test]
fn test_load_conversation_for_resume_basic_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s1.jsonl");
    // TS-faithful DAG: user → assistant → user → assistant.
    // u2's parent is a1 so the leaf walker reconstructs the full
    // chain rather than stopping at the disconnected sub-tree.
    let body = format!(
        "{}\n{}\n{}\n{}\n",
        user_line("u1", "first prompt"),
        assistant_line(
            "a1",
            "u1",
            "first reply",
            "claude-sonnet-4-6",
            Some((10, 20))
        ),
        user_line_with_parent("u2", Some("a1"), "second prompt"),
        assistant_line(
            "a2",
            "u2",
            "second reply",
            "claude-sonnet-4-6",
            Some((30, 40))
        ),
    );
    std::fs::write(&path, body).unwrap();

    let recovered = load_conversation_for_resume(&path).expect("resume loads");

    // 4 messages total — alternating user/assistant.
    assert_eq!(recovered.messages.len(), 4);
    assert!(matches!(recovered.messages[0], Message::User(_)));
    assert!(matches!(recovered.messages[1], Message::Assistant(_)));
    assert!(matches!(recovered.messages[2], Message::User(_)));
    assert!(matches!(recovered.messages[3], Message::Assistant(_)));

    // turn_count counts assistant entries.
    assert_eq!(recovered.turn_count, 2);

    // Latest model wins (both are the same here).
    assert_eq!(recovered.model, "claude-sonnet-4-6");

    // Token aggregation across the two assistant turns.
    assert_eq!(recovered.total_input_tokens, 40);
    assert_eq!(recovered.total_output_tokens, 60);

    // No sidechain in this transcript.
    assert!(!recovered.has_sidechain);
    assert!(recovered.plan_slug.is_none());
}

#[test]
fn test_load_conversation_for_resume_skips_metadata_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s2.jsonl");
    // Metadata-typed entries that should be skipped during recovery.
    let metadata_lines: Vec<String> = [
        "custom-title",
        "tag",
        "last-prompt",
        "summary",
        "cost-summary",
    ]
    .iter()
    .map(|t| {
        serde_json::to_string(&json!({
            "type": t,
            "sessionId": "s2",
        }))
        .unwrap()
    })
    .collect();
    let body = format!(
        "{}\n{}\n{}\n{}\n",
        metadata_lines.join("\n"),
        user_line("u1", "the only real prompt"),
        assistant_line("a1", "u1", "reply", "claude-sonnet-4-6", None),
        // Trailing blank + metadata to confirm robustness.
        "",
    );
    std::fs::write(&path, body).unwrap();

    let recovered = load_conversation_for_resume(&path).expect("resume loads");

    // Only the user + assistant entries materialize as messages.
    assert_eq!(
        recovered.messages.len(),
        2,
        "metadata + blank lines should be skipped",
    );
    assert_eq!(recovered.turn_count, 1);
}

#[test]
fn test_load_conversation_for_resume_sidechain_flag_and_filter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s3.jsonl");
    // One real exchange plus a sidechain entry that must be excluded
    // from messages but flip `has_sidechain`.
    let sidechain = serde_json::to_string(&json!({
        "type": "user",
        "uuid": "side1",
        "sessionId": "s3",
        "isSidechain": true,
        "message": {"role": "user", "content": "subagent prompt"},
    }))
    .unwrap();
    let body = format!(
        "{}\n{}\n{}\n",
        user_line("u1", "main prompt"),
        sidechain,
        assistant_line("a1", "u1", "main reply", "claude-sonnet-4-6", None),
    );
    std::fs::write(&path, body).unwrap();

    let recovered = load_conversation_for_resume(&path).expect("resume loads");
    assert!(recovered.has_sidechain, "sidechain flag should flip");
    assert_eq!(
        recovered.messages.len(),
        2,
        "sidechain entry must not appear in main messages: got {} msgs",
        recovered.messages.len(),
    );
}

#[test]
fn test_load_conversation_for_resume_plan_slug_extracted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s4.jsonl");
    // Embed a slug field on a transcript entry — recovery extracts the
    // first non-empty slug it sees.
    let with_slug = serde_json::to_string(&json!({
        "type": "user",
        "uuid": "u1",
        "sessionId": "s4",
        "isSidechain": false,
        "slug": "rebuild-cache",
        "message": {"role": "user", "content": "kick off plan"},
    }))
    .unwrap();
    let body = format!(
        "{}\n{}\n",
        with_slug,
        assistant_line("a1", "u1", "ack", "claude-sonnet-4-6", None),
    );
    std::fs::write(&path, body).unwrap();

    let recovered = load_conversation_for_resume(&path).expect("resume loads");
    assert_eq!(recovered.plan_slug.as_deref(), Some("rebuild-cache"));
}

#[test]
fn test_load_conversation_for_resume_missing_file_errors() {
    let err = load_conversation_for_resume(Path::new("/no/such/transcript.jsonl"))
        .expect_err("missing transcript should error");
    let msg = err.to_string();
    assert!(
        msg.contains("transcript not found"),
        "unexpected error message: {msg}",
    );
}

#[test]
fn test_load_conversation_for_resume_invalid_lines_skipped() {
    // Malformed JSONL lines must not abort the whole resume — recovery
    // is a best-effort path used after a crash.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s5.jsonl");
    let body = format!(
        "{}\nthis is not json at all\n{{\"type\":\"user\",\"uuid\":\"\",\n{}\n",
        user_line("u1", "good prompt"),
        assistant_line("a1", "u1", "good reply", "claude-sonnet-4-6", Some((5, 5))),
    );
    std::fs::write(&path, body).unwrap();

    let recovered = load_conversation_for_resume(&path).expect("resume tolerates bad lines");
    assert_eq!(recovered.messages.len(), 2);
    assert_eq!(recovered.turn_count, 1);
    assert_eq!(recovered.total_input_tokens, 5);
    assert_eq!(recovered.total_output_tokens, 5);
}

#[test]
fn test_load_conversation_for_resume_latest_model_wins() {
    // Two assistant turns from different models — recovered.model is
    // the latest non-empty one (per recovery.rs "latest wins"). DAG
    // links u2 → a1 so the leaf walker reconstructs both turns.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s6.jsonl");
    let body = format!(
        "{}\n{}\n{}\n{}\n",
        user_line("u1", "p1"),
        assistant_line("a1", "u1", "r1", "claude-sonnet-4-6", None),
        user_line_with_parent("u2", Some("a1"), "p2"),
        assistant_line("a2", "u2", "r2", "claude-opus-4-7", None),
    );
    std::fs::write(&path, body).unwrap();

    let recovered = load_conversation_for_resume(&path).expect("resume loads");
    assert_eq!(recovered.model, "claude-opus-4-7");
    assert_eq!(recovered.turn_count, 2);
}

/// New: tool_use / tool_result blocks must round-trip on resume so
/// the resumed model sees its own prior tool calls. TS:
/// `deserializeMessages` preserves `tool_use` / `tool_result` content.
#[test]
fn test_load_conversation_for_resume_preserves_tool_blocks() {
    use coco_messages::AssistantContent;
    use coco_messages::LlmMessage;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s7.jsonl");

    // Assistant message with a text part + tool_call block. Stored
    // shape mirrors what `engine_finalize_turn::record_transcript_tail`
    // writes — `serde_json::to_value(&assistant.message)` on an
    // `LlmMessage::Assistant`.
    let assistant_msg_with_tool = json!({
        "type": "assistant",
        "uuid": "a1",
        "parentUuid": "u1",
        "sessionId": "s7",
        "isSidechain": false,
        "timestamp": "2025-01-15T10:00:01Z",
        "model": "claude-sonnet-4-6",
        "message": {
            "role": "assistant",
            "content": [
                {"type": "text", "text": "let me check"},
                {
                    "type": "tool-call",
                    "toolCallId": "call_1",
                    "toolName": "Read",
                    "input": {"file_path": "/tmp/foo.txt"}
                }
            ]
        },
    });
    let body = format!(
        "{}\n{}\n",
        user_line("u1", "read foo"),
        serde_json::to_string(&assistant_msg_with_tool).unwrap(),
    );
    std::fs::write(&path, body).unwrap();

    let recovered = load_conversation_for_resume(&path).expect("resume loads");
    assert_eq!(recovered.messages.len(), 2);

    // Assistant message must round-trip both the text and the
    // tool_call so resumed turns can match the tool_result against
    // a real prior tool_use.
    let Message::Assistant(assistant) = &recovered.messages[1] else {
        panic!("expected assistant message at index 1");
    };
    let LlmMessage::Assistant { content, .. } = &assistant.message else {
        panic!("expected assistant LlmMessage variant");
    };
    let has_tool_call = content
        .iter()
        .any(|c| matches!(c, AssistantContent::ToolCall(_)));
    assert!(
        has_tool_call,
        "tool_call must round-trip; got content: {content:?}"
    );
}
