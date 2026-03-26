use super::*;
use cocode_config::json_config::ExtraMarketplaceConfig;
use cocode_config::json_config::MarketplaceSourceConfig;
use cocode_inference::AssistantContentPart;
use cocode_protocol::ToolName;

#[test]
fn test_turn_result_from_loop_result() {
    let loop_result = LoopResult::completed(
        3,
        1000,
        500,
        "Hello!".to_string(),
        vec![AssistantContentPart::text("Hello!")],
    );

    let turn = TurnResult::from_loop_result(&loop_result);
    assert_eq!(turn.final_text, "Hello!");
    assert_eq!(turn.turns_completed, 3);
    assert_eq!(turn.usage.input_tokens, 1000);
    assert_eq!(turn.usage.output_tokens, 500);
    assert!(turn.is_complete);
}

#[test]
fn test_turn_result_serde() {
    let turn = TurnResult {
        final_text: "test".to_string(),
        turns_completed: 5,
        usage: TokenUsage::new(100, 50),
        has_pending_tools: false,
        is_complete: true,
        stop_reason: StopReason::ModelStopSignal,
    };

    let json = serde_json::to_string(&turn).expect("serialize");
    let parsed: TurnResult = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(parsed.final_text, turn.final_text);
    assert_eq!(parsed.turns_completed, turn.turns_completed);
    assert_eq!(parsed.usage.input_tokens, turn.usage.input_tokens);
}

#[test]
fn test_convert_extra_marketplaces_all_variants() {
    let extras = vec![
        ExtraMarketplaceConfig {
            name: "gh-market".to_string(),
            source: MarketplaceSourceConfig::Github {
                repo: "owner/repo".to_string(),
                git_ref: Some("main".to_string()),
            },
            auto_update: true,
        },
        ExtraMarketplaceConfig {
            name: "git-market".to_string(),
            source: MarketplaceSourceConfig::Git {
                url: "https://example.com/repo.git".to_string(),
                git_ref: None,
            },
            auto_update: false,
        },
        ExtraMarketplaceConfig {
            name: "dir-market".to_string(),
            source: MarketplaceSourceConfig::Directory {
                path: "/tmp/plugins".to_string(),
            },
            auto_update: false,
        },
        ExtraMarketplaceConfig {
            name: "url-market".to_string(),
            source: MarketplaceSourceConfig::Url {
                url: "https://example.com/marketplace.json".to_string(),
            },
            auto_update: true,
        },
    ];

    let result = convert_extra_marketplaces(&extras);
    assert_eq!(result.len(), 4);

    assert_eq!(result[0].name, "gh-market");
    assert!(result[0].auto_update);
    assert!(matches!(
        &result[0].source,
        cocode_plugin::MarketplaceSource::Github { repo, git_ref }
        if repo == "owner/repo" && *git_ref == Some("main".to_string())
    ));

    assert_eq!(result[1].name, "git-market");
    assert!(!result[1].auto_update);
    assert!(matches!(
        &result[1].source,
        cocode_plugin::MarketplaceSource::Git { url, .. }
        if url == "https://example.com/repo.git"
    ));

    assert_eq!(result[2].name, "dir-market");
    assert!(matches!(
        &result[2].source,
        cocode_plugin::MarketplaceSource::Directory { path }
        if path == std::path::Path::new("/tmp/plugins")
    ));

    assert_eq!(result[3].name, "url-market");
    assert!(result[3].auto_update);
    assert!(matches!(
        &result[3].source,
        cocode_plugin::MarketplaceSource::Url { url }
        if url == "https://example.com/marketplace.json"
    ));
}

#[test]
fn test_convert_extra_marketplaces_empty() {
    let result = convert_extra_marketplaces(&[]);
    assert!(result.is_empty());
}

#[test]
fn test_marketplace_source_display() {
    assert_eq!(
        marketplace_source_display(&cocode_plugin::MarketplaceSource::Github {
            repo: "owner/repo".to_string(),
            git_ref: None,
        }),
        ("github".to_string(), "owner/repo".to_string())
    );

    assert_eq!(
        marketplace_source_display(&cocode_plugin::MarketplaceSource::Git {
            url: "https://git.example.com/repo".to_string(),
            git_ref: Some("v1".to_string()),
        }),
        (
            "git".to_string(),
            "https://git.example.com/repo".to_string()
        )
    );

    assert_eq!(
        marketplace_source_display(&cocode_plugin::MarketplaceSource::Directory {
            path: std::path::PathBuf::from("/tmp/dir"),
        }),
        ("directory".to_string(), "/tmp/dir".to_string())
    );

    assert_eq!(
        marketplace_source_display(&cocode_plugin::MarketplaceSource::Url {
            url: "https://example.com/m.json".to_string(),
        }),
        ("url".to_string(), "https://example.com/m.json".to_string())
    );
}

// ============================================================================
// File Tracker State Tests
// ============================================================================

#[test]
fn test_prune_reminder_file_tracker_for_turn_boundary() {
    use cocode_protocol::FileReadKind;
    use cocode_tools::FileReadState;
    use std::path::PathBuf;
    use std::time::SystemTime;

    // Create some file tracker state entries with different turn numbers
    let mut state: Vec<(PathBuf, FileReadState)> = vec![
        (
            PathBuf::from("/file1.txt"),
            FileReadState {
                content: Some("content1".to_string()),
                timestamp: SystemTime::now(),
                file_mtime: None,
                content_hash: None,
                offset: None,
                limit: None,
                kind: FileReadKind::FullContent,
                access_count: 1,
                read_turn: 1,
            },
        ),
        (
            PathBuf::from("/file2.txt"),
            FileReadState {
                content: Some("content2".to_string()),
                timestamp: SystemTime::now(),
                file_mtime: None,
                content_hash: None,
                offset: None,
                limit: None,
                kind: FileReadKind::FullContent,
                access_count: 1,
                read_turn: 2,
            },
        ),
        (
            PathBuf::from("/file3.txt"),
            FileReadState {
                content: Some("content3".to_string()),
                timestamp: SystemTime::now(),
                file_mtime: None,
                content_hash: None,
                offset: None,
                limit: None,
                kind: FileReadKind::FullContent,
                access_count: 1,
                read_turn: 3,
            },
        ),
    ];

    // Prune entries at or after turn 2
    state.retain(|(_, s)| s.read_turn < 2);

    // Should only have file1.txt left (turn 1 < 2)
    assert_eq!(state.len(), 1);
    assert_eq!(state[0].0, PathBuf::from("/file1.txt"));
}

#[test]
fn test_rebuild_file_tracker_from_modifiers() {
    use cocode_protocol::ContextModifier;
    use cocode_protocol::FileReadKind;
    use cocode_system_reminder::build_file_read_state_from_modifiers;
    use std::path::Path;
    use std::path::PathBuf;

    // Create tool call modifiers
    let modifiers: Vec<ContextModifier> = vec![
        ContextModifier::FileRead {
            path: PathBuf::from("/file1.txt"),
            content: "content1".to_string(),
            file_mtime_ms: None,
            offset: None,
            limit: None,
            read_kind: FileReadKind::FullContent,
        },
        ContextModifier::FileRead {
            path: PathBuf::from("/file2.txt"),
            content: "partial".to_string(),
            file_mtime_ms: Some(1000),
            offset: Some(10),
            limit: Some(20),
            read_kind: FileReadKind::PartialContent,
        },
    ];

    // Build from modifiers
    let tool_calls = vec![(ToolName::Read.as_str(), modifiers.as_slice(), 1, true)];
    let state = build_file_read_state_from_modifiers(tool_calls.into_iter(), 10);

    assert_eq!(state.len(), 2);

    // Find file1 (full content)
    let file1 = state.iter().find(|(p, _)| p == Path::new("/file1.txt"));
    assert!(file1.is_some());
    let (_, state1) = file1.unwrap();
    assert!(!state1.is_partial());
    assert_eq!(state1.content.as_deref(), Some("content1"));

    // Find file2 (partial content)
    let file2 = state.iter().find(|(p, _)| p == Path::new("/file2.txt"));
    assert!(file2.is_some());
    let (_, state2) = file2.unwrap();
    assert!(state2.is_partial());
    assert_eq!(state2.offset, Some(10));
    assert_eq!(state2.limit, Some(20));
}

#[test]
fn test_file_tracker_state_prefers_newer_turns() {
    use cocode_protocol::ContextModifier;
    use cocode_protocol::FileReadKind;
    use cocode_system_reminder::build_file_read_state_from_modifiers;
    use std::path::PathBuf;

    // Same file read in two different turns
    let modifiers1: Vec<ContextModifier> = vec![ContextModifier::FileRead {
        path: PathBuf::from("/file.txt"),
        content: "old content".to_string(),
        file_mtime_ms: None,
        offset: None,
        limit: None,
        read_kind: FileReadKind::FullContent,
    }];

    let modifiers2: Vec<ContextModifier> = vec![ContextModifier::FileRead {
        path: PathBuf::from("/file.txt"),
        content: "new content".to_string(),
        file_mtime_ms: None,
        offset: None,
        limit: None,
        read_kind: FileReadKind::FullContent,
    }];

    let tool_calls = vec![
        (ToolName::Read.as_str(), modifiers1.as_slice(), 1, true),
        (ToolName::Read.as_str(), modifiers2.as_slice(), 2, true),
    ];

    let state = build_file_read_state_from_modifiers(tool_calls.into_iter(), 10);

    // Should only have one entry (the newer one)
    assert_eq!(state.len(), 1);
    assert_eq!(state[0].1.content.as_deref(), Some("new content"));
    assert_eq!(state[0].1.read_turn, 2);
}
