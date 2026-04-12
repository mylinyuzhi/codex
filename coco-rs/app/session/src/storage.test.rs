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
        "\n{\"not_a_valid_entry\": true}\n\n{\"type\":\"user\",\"uuid\":\"u1\",\"session_id\":\"s\",\"cwd\":\"\",\"timestamp\":\"\",\"is_sidechain\":false}\n",
    )
    .unwrap();

    let entries = load_entries_from_file(&path).unwrap();
    // Empty lines skipped, malformed kept as Unknown, valid transcript parsed.
    assert_eq!(entries.len(), 2);
    assert!(matches!(&entries[0], Entry::Unknown(_)));
    assert!(matches!(&entries[1], Entry::Transcript(_)));
}
