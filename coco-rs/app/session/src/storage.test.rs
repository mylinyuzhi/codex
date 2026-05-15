use super::*;
use serde_json::json;

fn make_user_entry(uuid: &str, session_id: &str, text: &str) -> TranscriptEntry {
    TranscriptEntry {
        entry_type: "user".to_string(),
        uuid: uuid.to_string(),
        parent_uuid: None,
        session_id: session_id.to_string(),
        cwd: "/tmp/project".to_string(),
        timestamp: "2025-01-15T10:00:00Z".to_string(),
        version: Some("1.0.0".to_string()),
        git_branch: Some("main".to_string()),
        is_sidechain: false,
        message: Some(json!({
            "role": "user",
            "content": text,
        })),
        usage: None,
        model: None,
        cost_usd: None,
        extra: serde_json::Map::new(),
    }
}

fn make_assistant_entry(uuid: &str, parent_uuid: &str, session_id: &str) -> TranscriptEntry {
    TranscriptEntry {
        entry_type: "assistant".to_string(),
        uuid: uuid.to_string(),
        parent_uuid: Some(parent_uuid.to_string()),
        session_id: session_id.to_string(),
        cwd: "/tmp/project".to_string(),
        timestamp: "2025-01-15T10:00:01Z".to_string(),
        version: Some("1.0.0".to_string()),
        git_branch: Some("main".to_string()),
        is_sidechain: false,
        message: Some(json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "Sure, I can help."}],
        })),
        usage: Some(TranscriptUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: None,
            cache_creation_tokens: None,
        }),
        model: Some("claude-sonnet-4-6".into()),
        cost_usd: Some(0.003),
        extra: serde_json::Map::new(),
    }
}

#[test]
fn test_append_and_load_entries() {
    let dir = tempfile::tempdir().unwrap();
    let store = TranscriptStore::new(dir.path().to_path_buf());
    let sid = "test-session-001";

    let user = make_user_entry("u1", sid, "Hello world");
    let assistant = make_assistant_entry("a1", "u1", sid);

    store.append_message(sid, &user).unwrap();
    store.append_message(sid, &assistant).unwrap();

    let entries = store.load_entries(sid).unwrap();
    assert_eq!(entries.len(), 2);

    // Verify round-trip fidelity.
    match &entries[0] {
        Entry::Transcript(t) => {
            assert_eq!(t.entry_type, "user");
            assert_eq!(t.uuid, "u1");
        }
        other => panic!("expected Transcript, got {other:?}"),
    }
    match &entries[1] {
        Entry::Transcript(t) => {
            assert_eq!(t.entry_type, "assistant");
            assert_eq!(t.parent_uuid.as_deref(), Some("u1"));
        }
        other => panic!("expected Transcript, got {other:?}"),
    }
}

#[test]
fn test_metadata_entries_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let store = TranscriptStore::new(dir.path().to_path_buf());
    let sid = "meta-session";

    // Write a user message so the file has content.
    let user = make_user_entry("u1", sid, "Fix the bug");
    store.append_message(sid, &user).unwrap();

    // Append metadata entries.
    store
        .append_metadata(
            sid,
            &MetadataEntry::CustomTitle {
                session_id: sid.to_string(),
                custom_title: "Bug fix session".to_string(),
            },
        )
        .unwrap();
    store
        .append_metadata(
            sid,
            &MetadataEntry::Tag {
                session_id: sid.to_string(),
                tag: "bugfix".to_string(),
            },
        )
        .unwrap();
    store
        .append_metadata(
            sid,
            &MetadataEntry::LastPrompt {
                session_id: sid.to_string(),
                last_prompt: "Fix the bug".to_string(),
            },
        )
        .unwrap();

    // Load metadata.
    let meta = store.read_metadata(sid).unwrap();
    assert_eq!(meta.session_id, sid);
    assert_eq!(meta.custom_title.as_deref(), Some("Bug fix session"));
    assert_eq!(meta.tag.as_deref(), Some("bugfix"));
    assert_eq!(meta.last_prompt.as_deref(), Some("Fix the bug"));
    assert_eq!(meta.first_prompt, "Fix the bug");
    assert_eq!(meta.message_count, 1); // Only the user message.
    assert!(!meta.is_sidechain);
}

#[test]
fn test_content_replacement_records_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let store = TranscriptStore::new(dir.path().to_path_buf());
    let sid = "content-replacements";

    let records = vec![ContentReplacementRecord::tool_result(
        "toolu_1",
        "<persisted-output>\npreview\n</persisted-output>",
    )];
    store.insert_content_replacement(sid, &records).unwrap();

    assert_eq!(
        store.tool_results_session_dir(sid),
        dir.path().join(sid).join("tool-results")
    );
    let loaded = store.load_content_replacements(sid).unwrap();
    assert_eq!(loaded, records);
}

#[test]
fn test_cleanup_tool_results_older_than_removes_expired_files_and_empty_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let store = TranscriptStore::new(dir.path().to_path_buf());
    let tool_results = store.tool_results_session_dir("session-a");
    let nested = tool_results.join("tool-dir");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(tool_results.join("call-1.txt"), "large output").unwrap();
    std::fs::write(nested.join("chunk-1.bin"), [1, 2, 3]).unwrap();

    std::thread::sleep(std::time::Duration::from_millis(5));
    let removed = store
        .cleanup_tool_results_older_than(std::time::Duration::ZERO)
        .unwrap();

    assert_eq!(removed, 2);
    assert!(!tool_results.exists());
    assert!(!dir.path().join("session-a").exists());
}

#[test]
fn test_cleanup_tool_results_older_than_keeps_recent_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = TranscriptStore::new(dir.path().to_path_buf());
    let tool_results = store.tool_results_session_dir("session-b");
    std::fs::create_dir_all(&tool_results).unwrap();
    let output = tool_results.join("call-1.txt");
    std::fs::write(&output, "large output").unwrap();

    let removed = store
        .cleanup_tool_results_older_than(std::time::Duration::from_secs(24 * 60 * 60))
        .unwrap();

    assert_eq!(removed, 0);
    assert!(output.exists());
    assert!(tool_results.exists());
}

#[test]
fn test_list_sessions_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let store = TranscriptStore::new(dir.path().to_path_buf());

    // Create two sessions with a small delay so mtime differs.
    let user_a = make_user_entry("u1", "session-a", "First session");
    store.append_message("session-a", &user_a).unwrap();

    // Touch file to ensure different mtime.
    std::thread::sleep(std::time::Duration::from_millis(50));

    let user_b = make_user_entry("u2", "session-b", "Second session");
    store.append_message("session-b", &user_b).unwrap();

    let sessions = store.list_sessions().unwrap();
    assert_eq!(sessions.len(), 2);
    // Newest first.
    assert_eq!(sessions[0].session_id, "session-b");
    assert_eq!(sessions[1].session_id, "session-a");
}

#[test]
fn test_load_transcript_messages_filters_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let store = TranscriptStore::new(dir.path().to_path_buf());
    let sid = "filter-session";

    let user = make_user_entry("u1", sid, "Hello");
    store.append_message(sid, &user).unwrap();
    store
        .append_metadata(
            sid,
            &MetadataEntry::CustomTitle {
                session_id: sid.to_string(),
                custom_title: "Test".to_string(),
            },
        )
        .unwrap();
    let assistant = make_assistant_entry("a1", "u1", sid);
    store.append_message(sid, &assistant).unwrap();

    let messages = store.load_transcript_messages(sid).unwrap();
    // Only transcript messages, not metadata.
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].entry_type, "user");
    assert_eq!(messages[1].entry_type, "assistant");
}

#[test]
fn test_exists_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let store = TranscriptStore::new(dir.path().to_path_buf());
    let sid = "del-session";

    assert!(!store.exists(sid));

    let user = make_user_entry("u1", sid, "To be deleted");
    store.append_message(sid, &user).unwrap();
    assert!(store.exists(sid));

    store.delete(sid).unwrap();
    assert!(!store.exists(sid));
}

#[test]
fn test_extract_text_content_string() {
    let entry = make_user_entry("u1", "s1", "Simple text prompt");
    let text = extract_text_content(&entry);
    assert_eq!(text, "Simple text prompt");
}

#[test]
fn test_extract_text_content_array() {
    let mut entry = make_user_entry("u1", "s1", "");
    entry.message = Some(json!({
        "role": "user",
        "content": [
            {"type": "tool_result", "text": "ignored"},
            {"type": "text", "text": "Actual prompt text"},
        ],
    }));
    let text = extract_text_content(&entry);
    assert_eq!(text, "Actual prompt text");
}

#[test]
fn test_truncate_prompt_long_text() {
    let long = "a".repeat(300);
    let truncated = truncate_prompt(&long);
    assert!(truncated.ends_with("..."));
    // 200 chars + "..."
    assert_eq!(truncated.len(), 203);
}

#[test]
fn test_load_nonexistent_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let store = TranscriptStore::new(dir.path().to_path_buf());
    let result = store.load_entries("nonexistent");
    assert!(result.is_err());
}

#[test]
fn test_empty_and_malformed_lines_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.jsonl");
    std::fs::write(
        &path,
        "\n{\"not_a_valid_entry\": true}\n\n{\"type\":\"user\",\"uuid\":\"u1\",\"sessionId\":\"s\",\"cwd\":\"\",\"timestamp\":\"\",\"isSidechain\":false}\n",
    )
    .unwrap();

    let entries = load_entries_from_file(&path).unwrap();
    // Empty lines skipped, malformed kept as Unknown, valid transcript parsed.
    assert_eq!(entries.len(), 2);
    assert!(matches!(&entries[0], Entry::Unknown(_)));
    assert!(matches!(&entries[1], Entry::Transcript(_)));
}

// ---------------------------------------------------------------------------
// Rewind support — file-history snapshot chain + marble-origami replay.
// These metadata entries persist into the JSONL alongside transcript
// messages. The rewind picker uses the snapshot chain to know which
// pre-edit file states it can restore to; marble-origami entries
// preserve context-collapse staging across resume.
// ---------------------------------------------------------------------------

#[test]
fn test_file_history_snapshot_round_trip_appends_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let store = TranscriptStore::new(dir.path().to_path_buf());
    let sid = "rewind-session";

    // Seed transcript with at least one message so the JSONL has shape.
    store
        .append_message(sid, &make_user_entry("u1", sid, "edit a"))
        .unwrap();

    // Two distinct messages each get one file-history snapshot.
    store
        .insert_file_history_snapshot(
            sid,
            "msg-1",
            json!({"files": {"a.txt": {"content": "v1"}}}),
            /*is_snapshot_update*/ false,
        )
        .unwrap();
    store
        .insert_file_history_snapshot(
            sid,
            "msg-2",
            json!({"files": {"a.txt": {"content": "v2"}}}),
            false,
        )
        .unwrap();

    let chain = store.load_file_history_snapshots(sid).unwrap();
    assert_eq!(chain.len(), 2, "expected one snapshot per message_id");
    // Order matches insertion order.
    assert_eq!(
        chain[0]
            .pointer("/files/a.txt/content")
            .and_then(|v| v.as_str()),
        Some("v1"),
    );
    assert_eq!(
        chain[1]
            .pointer("/files/a.txt/content")
            .and_then(|v| v.as_str()),
        Some("v2"),
    );
}

#[test]
fn test_file_history_snapshot_chain_last_wins_on_update() {
    // The rewind subsystem's `tracked_edit` flow rewrites a not-yet-
    // flushed snapshot in place — `is_snapshot_update = true` should
    // overwrite the prior entry for the same `message_id` rather than
    // appending a new row. This is the load-bearing invariant for the
    // rewind picker (TS `buildFileHistorySnapshotChain`).
    let dir = tempfile::tempdir().unwrap();
    let store = TranscriptStore::new(dir.path().to_path_buf());
    let sid = "update-session";
    store
        .append_message(sid, &make_user_entry("u1", sid, "edit"))
        .unwrap();

    store
        .insert_file_history_snapshot(sid, "msg-A", json!({"version": "first"}), false)
        .unwrap();
    store
        .insert_file_history_snapshot(sid, "msg-B", json!({"version": "first-B"}), false)
        .unwrap();
    // Update msg-A in place — new snapshot, same message_id.
    store
        .insert_file_history_snapshot(
            sid,
            "msg-A",
            json!({"version": "second"}),
            /*is_snapshot_update*/ true,
        )
        .unwrap();

    let chain = store.load_file_history_snapshots(sid).unwrap();
    assert_eq!(
        chain.len(),
        2,
        "update should overwrite, not append (got {chain:?})",
    );
    // msg-A's slot now holds the updated snapshot…
    assert_eq!(
        chain[0].pointer("/version").and_then(|v| v.as_str()),
        Some("second"),
    );
    // …and msg-B is unchanged at its position.
    assert_eq!(
        chain[1].pointer("/version").and_then(|v| v.as_str()),
        Some("first-B"),
    );
}

#[test]
fn test_file_history_snapshot_chain_unknown_id_ignored() {
    // An update for a `message_id` that hasn't been recorded yet
    // should be treated as a fresh insert (TS parity — the chain
    // builder only consults the index for known ids).
    let entries = vec![
        Entry::Metadata(MetadataEntry::FileHistorySnapshot {
            message_id: "msg-A".into(),
            snapshot: json!({"v": "a1"}),
            is_snapshot_update: false,
        }),
        // is_snapshot_update=true for an *unknown* id — falls through
        // and gets pushed as a new entry.
        Entry::Metadata(MetadataEntry::FileHistorySnapshot {
            message_id: "msg-Z".into(),
            snapshot: json!({"v": "z-update"}),
            is_snapshot_update: true,
        }),
    ];
    let chain = build_file_history_snapshot_chain(&entries);
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].pointer("/v").and_then(|v| v.as_str()), Some("a1"));
    assert_eq!(
        chain[1].pointer("/v").and_then(|v| v.as_str()),
        Some("z-update"),
    );
}

#[test]
fn test_marble_origami_entries_filtered_by_session() {
    // Both commit and snapshot entries are session-scoped via the
    // `sessionId` field on the payload. The loader must drop entries
    // tagged with a different session_id (e.g. when transcripts get
    // forked / merged).
    let dir = tempfile::tempdir().unwrap();
    let store = TranscriptStore::new(dir.path().to_path_buf());
    let sid = "mo-session";
    store
        .append_message(sid, &make_user_entry("u1", sid, "p"))
        .unwrap();

    // Two commits for our session, one for a different session.
    store
        .append_marble_origami_commit(sid, json!({"sessionId": sid, "id": "c1"}))
        .unwrap();
    store
        .append_marble_origami_commit(sid, json!({"sessionId": "other", "id": "c-other"}))
        .unwrap();
    store
        .append_marble_origami_commit(sid, json!({"sessionId": sid, "id": "c2"}))
        .unwrap();

    // Two snapshots — last-wins by session_id; only the our-session one counts.
    store
        .append_marble_origami_snapshot(sid, json!({"sessionId": "other", "v": "ignore"}))
        .unwrap();
    store
        .append_marble_origami_snapshot(sid, json!({"sessionId": sid, "v": "keep-1"}))
        .unwrap();
    store
        .append_marble_origami_snapshot(sid, json!({"sessionId": sid, "v": "keep-2"}))
        .unwrap();

    let (commits, snapshot) = store.load_marble_origami_entries(sid).unwrap();
    assert_eq!(commits.len(), 2, "off-session commit must be dropped");
    assert_eq!(
        commits[0].pointer("/id").and_then(|v| v.as_str()),
        Some("c1")
    );
    assert_eq!(
        commits[1].pointer("/id").and_then(|v| v.as_str()),
        Some("c2")
    );

    let snap = snapshot.expect("snapshot should exist");
    assert_eq!(
        snap.pointer("/v").and_then(|v| v.as_str()),
        Some("keep-2"),
        "last-wins on snapshot for matching session",
    );
}

/// Wire-format regression: TS Claude Code's JSONL transcript uses
/// camelCase keys (`parentUuid`, `sessionId`, `isSidechain`, `gitBranch`,
/// `costUsd`, `inputTokens`, `outputTokens`, `cacheReadTokens`, etc.).
/// If anyone "corrects" our serde to snake_case, this test fails — and
/// every prior on-disk transcript becomes unreadable for the next
/// resume. Lock the wire shape here.
#[test]
fn test_transcript_entry_serializes_with_camelcase_keys() {
    let entry = TranscriptEntry {
        entry_type: "assistant".into(),
        uuid: "uu".into(),
        parent_uuid: Some("pp".into()),
        session_id: "ss".into(),
        cwd: "/tmp".into(),
        timestamp: "2025-01-15T10:00:00Z".into(),
        version: Some("1.0".into()),
        git_branch: Some("main".into()),
        is_sidechain: true,
        message: Some(json!({"role": "assistant", "content": []})),
        usage: Some(TranscriptUsage {
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: Some(3),
            cache_creation_tokens: Some(4),
        }),
        model: Some("claude-sonnet-4-6".into()),
        cost_usd: Some(0.5),
        extra: serde_json::Map::new(),
    };
    let v = serde_json::to_value(&entry).unwrap();
    // Top-level transcript fields.
    assert!(
        v.get("parentUuid").is_some(),
        "parentUuid must be camelCase"
    );
    assert!(v.get("sessionId").is_some());
    assert!(v.get("isSidechain").is_some());
    assert!(v.get("gitBranch").is_some());
    assert!(v.get("costUsd").is_some());
    // Snake_case versions must be ABSENT — sole source of truth is camelCase.
    assert!(v.get("parent_uuid").is_none());
    assert!(v.get("session_id").is_none());
    assert!(v.get("is_sidechain").is_none());
    // Nested usage block.
    let usage = v.get("usage").expect("usage present");
    assert!(usage.get("inputTokens").is_some());
    assert!(usage.get("outputTokens").is_some());
    assert!(usage.get("cacheReadTokens").is_some());
    assert!(usage.get("cacheCreationTokens").is_some());
}

/// Same check for `MetadataEntry` — TS writes
/// `{type:"custom-title", sessionId, customTitle}` (kebab-case
/// discriminator + camelCase payload).
#[test]
fn test_metadata_entry_serializes_with_camelcase_payload() {
    let m = MetadataEntry::CustomTitle {
        session_id: "ss".into(),
        custom_title: "My Bug Hunt".into(),
    };
    let v = serde_json::to_value(&m).unwrap();
    assert_eq!(v.get("type").and_then(|t| t.as_str()), Some("custom-title"));
    assert_eq!(v.get("sessionId").and_then(|t| t.as_str()), Some("ss"));
    assert_eq!(
        v.get("customTitle").and_then(|t| t.as_str()),
        Some("My Bug Hunt"),
    );
    assert!(
        v.get("custom_title").is_none(),
        "snake_case payload must be gone"
    );
}

#[test]
fn test_content_replacement_serializes_ts_shape() {
    let m = MetadataEntry::ContentReplacement {
        record: ContentReplacementRecord::tool_result("toolu_1", "replacement"),
    };
    let v = serde_json::to_value(&m).unwrap();
    assert_eq!(
        v.get("type").and_then(|t| t.as_str()),
        Some("content-replacement")
    );
    assert_eq!(v.get("kind").and_then(|t| t.as_str()), Some("tool-result"));
    assert_eq!(v.get("toolUseId").and_then(|t| t.as_str()), Some("toolu_1"));
    assert_eq!(
        v.get("replacement").and_then(|t| t.as_str()),
        Some("replacement")
    );
}
