use super::*;

fn make_user_message(text: &str) -> Message {
    Message::User(coco_types::UserMessage {
        message: coco_types::LlmMessage::User {
            content: vec![coco_types::UserContent::Text(
                vercel_ai_provider::TextPart::new(text.to_string()),
            )],
            provider_options: None,
        },
        uuid: uuid::Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: false,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: None,
    })
}

fn make_assistant_message(text: &str) -> Message {
    Message::Assistant(coco_types::AssistantMessage {
        message: coco_types::LlmMessage::Assistant {
            content: vec![coco_types::AssistantContent::Text(
                vercel_ai_provider::TextPart::new(text.to_string()),
            )],
            provider_options: None,
        },
        uuid: uuid::Uuid::new_v4(),
        model: "test-model".to_string(),
        stop_reason: None,
        usage: None,
        cost_usd: None,
        request_id: None,
        api_error: None,
    })
}

#[test]
fn test_compact_session_memory_produces_summary() {
    let messages = vec![
        make_user_message("Hello, help me with this code"),
        make_assistant_message("Sure, I can help you refactor that module"),
        make_user_message("Great, let's start with the parser"),
        make_assistant_message("I'll restructure the parser into smaller functions"),
    ];
    let session_memory = "## Key Decisions\n- Refactored parser into smaller functions\n\n## Context\n- User working on parser module";
    let config = SessionMemoryCompactConfig::default();

    let result = compact_session_memory(&messages, session_memory, &config)
        .expect("should not error")
        .expect("should produce a result");

    assert!(
        !result.summary_messages.is_empty(),
        "should have summary messages"
    );
    assert!(result.pre_compact_tokens > 0);
    // Boundary marker should be in the dedicated field, not in summary_messages
    assert!(matches!(
        result.boundary_marker,
        Message::System(coco_types::SystemMessage::CompactBoundary(_))
    ));
    // summary_messages should contain only the user summary, not the boundary
    assert!(
        result
            .summary_messages
            .iter()
            .all(|m| matches!(m, Message::User(_)))
    );
}

#[test]
fn test_compact_session_memory_empty_returns_none() {
    let messages = vec![make_user_message("hello")];
    let config = SessionMemoryCompactConfig::default();

    let result = compact_session_memory(&messages, "", &config).expect("should not error");
    assert!(result.is_none(), "empty session memory should return None");

    let result2 = compact_session_memory(&messages, "   \n  ", &config).expect("should not error");
    assert!(
        result2.is_none(),
        "whitespace-only session memory should return None"
    );
}

#[test]
fn test_select_memories_for_compaction_picks_stale() {
    let entries = vec![
        (
            "recent.md".to_string(),
            MemoryMetadata {
                staleness_days: 1,
                access_count: 10,
            },
        ),
        (
            "old.md".to_string(),
            MemoryMetadata {
                staleness_days: 30,
                access_count: 2,
            },
        ),
        (
            "medium.md".to_string(),
            MemoryMetadata {
                staleness_days: 15,
                access_count: 5,
            },
        ),
    ];

    let to_compact = select_memories_for_compaction(&entries, /*max_to_keep*/ 2);
    assert_eq!(to_compact.len(), 1);
    assert_eq!(
        to_compact[0], "old.md",
        "oldest entry should be selected first"
    );
}

#[test]
fn test_select_memories_under_limit_returns_empty() {
    let entries = vec![
        (
            "a.md".to_string(),
            MemoryMetadata {
                staleness_days: 1,
                access_count: 1,
            },
        ),
        (
            "b.md".to_string(),
            MemoryMetadata {
                staleness_days: 2,
                access_count: 1,
            },
        ),
    ];
    let to_compact = select_memories_for_compaction(&entries, /*max_to_keep*/ 5);
    assert!(to_compact.is_empty());
}

#[test]
fn test_merge_similar_memories_deduplicates() {
    let memories = vec![
        (
            "parser_v1.md".to_string(),
            "Use recursive descent\nHandle errors gracefully".to_string(),
        ),
        (
            "parser_v2.md".to_string(),
            "Use recursive descent\nAdd streaming support".to_string(),
        ),
    ];

    let merged = merge_similar_memories(&memories);
    assert_eq!(merged.len(), 1, "should merge into one entry");
    let (name, content) = &merged[0];
    assert_eq!(name, "parser");
    // "Use recursive descent" should appear only once
    assert_eq!(
        content.matches("Use recursive descent").count(),
        1,
        "duplicate lines should be deduplicated"
    );
    assert!(content.contains("Handle errors gracefully"));
    assert!(content.contains("Add streaming support"));
}

#[test]
fn test_merge_similar_memories_keeps_unique() {
    let memories = vec![
        (
            "auth_config.md".to_string(),
            "OAuth2 setup notes".to_string(),
        ),
        ("parser_rules.md".to_string(), "Grammar rules".to_string()),
    ];

    let merged = merge_similar_memories(&memories);
    assert_eq!(merged.len(), 2, "different prefixes should not be merged");
}

#[test]
fn test_extract_name_prefix() {
    assert_eq!(extract_name_prefix("parser_v1.md"), "parser");
    assert_eq!(extract_name_prefix("auth-config.md"), "auth");
    assert_eq!(extract_name_prefix("simple.md"), "simple");
    assert_eq!(extract_name_prefix("no_ext"), "no");
}
