use super::*;
use cocode_protocol::ToolName;

#[test]
fn test_build_read_state_from_modifier() {
    // Full content read
    let state = build_read_state_from_modifier(
        "file content".to_string(),
        None,
        1,
        None,
        None,
        FileReadKind::FullContent,
    );
    assert!(state.is_some());
    let state = state.unwrap();
    assert_eq!(state.kind, FileReadKind::FullContent);
    assert!(state.content.is_some());
    assert!(state.content_hash.is_some());

    // Partial content read
    let state = build_read_state_from_modifier(
        "partial".to_string(),
        None,
        2,
        Some(10i64),
        Some(20i64),
        FileReadKind::PartialContent,
    );
    assert!(state.is_some());
    let state = state.unwrap();
    assert_eq!(state.kind, FileReadKind::PartialContent);
    assert_eq!(state.offset, Some(10));
    assert_eq!(state.limit, Some(20));

    // Metadata-only read - should return None
    let state = build_read_state_from_modifier(
        String::new(),
        None,
        3,
        None,
        None,
        FileReadKind::MetadataOnly,
    );
    assert!(state.is_none());
}

#[test]
fn test_merge_file_read_state() {
    let base = vec![
        (
            PathBuf::from("/file1.txt"),
            FileReadState {
                content: Some("old content".to_string()),
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
                content: Some("content".to_string()),
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
    ];

    let incoming = vec![
        (
            PathBuf::from("/file1.txt"),
            FileReadState {
                content: Some("new content".to_string()),
                timestamp: SystemTime::now(),
                file_mtime: None,
                content_hash: None,
                offset: None,
                limit: None,
                kind: FileReadKind::FullContent,
                access_count: 1,
                read_turn: 3, // Newer
            },
        ),
        (
            PathBuf::from("/file3.txt"),
            FileReadState {
                content: Some("content 3".to_string()),
                timestamp: SystemTime::now(),
                file_mtime: None,
                content_hash: None,
                offset: None,
                limit: None,
                kind: FileReadKind::FullContent,
                access_count: 1,
                read_turn: 4,
            },
        ),
    ];

    let merged = merge_file_read_state(base, incoming);

    // Should have 3 entries
    assert_eq!(merged.len(), 3);

    // file1.txt should have turn 3 (newer)
    let file1 = merged
        .iter()
        .find(|(p, _)| p.to_str() == Some("/file1.txt"));
    assert!(file1.is_some());
    assert_eq!(file1.unwrap().1.read_turn, 3);
}

#[test]
fn test_build_file_read_state_from_modifiers() {
    let modifiers: Vec<ContextModifier> = vec![ContextModifier::FileRead {
        path: PathBuf::from("/file1.txt"),
        content: "content 1".to_string(),
        file_mtime_ms: None,
        offset: None,
        limit: None,
        read_kind: FileReadKind::FullContent,
    }];

    let tool_calls = vec![(ToolName::Read.as_str(), modifiers.as_slice(), 1, true)];

    let state = build_file_read_state_from_modifiers(tool_calls.into_iter(), 10);

    assert_eq!(state.len(), 1);
    assert_eq!(state[0].0, PathBuf::from("/file1.txt"));
    assert_eq!(state[0].1.read_turn, 1);
}

#[test]
fn test_build_file_state_prefers_newer_turn() {
    // When the same file is read in multiple turns, prefer the newer turn
    let modifiers1: Vec<ContextModifier> = vec![ContextModifier::FileRead {
        path: PathBuf::from("/tmp/a.rs"),
        content: "v1".to_string(),
        file_mtime_ms: Some(1000),
        offset: None,
        limit: None,
        read_kind: FileReadKind::FullContent,
    }];

    let modifiers2: Vec<ContextModifier> = vec![ContextModifier::FileRead {
        path: PathBuf::from("/tmp/a.rs"),
        content: "v2".to_string(),
        file_mtime_ms: Some(2000),
        offset: None,
        limit: None,
        read_kind: FileReadKind::FullContent,
    }];

    let tool_calls = vec![
        (ToolName::Read.as_str(), modifiers1.as_slice(), 1, true),
        (ToolName::Read.as_str(), modifiers2.as_slice(), 2, true),
    ];

    let states = build_file_read_state_from_modifiers(tool_calls.into_iter(), 10);
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].1.content.as_deref(), Some("v2"));
    assert_eq!(states[0].1.read_turn, 2);
}

#[test]
fn test_build_file_state_ignores_calls_without_modifiers() {
    // Tool calls without FileRead modifiers should be ignored
    let empty_modifiers: Vec<ContextModifier> = vec![];
    let tool_calls = vec![(ToolName::Read.as_str(), empty_modifiers.as_slice(), 1, true)];

    let states = build_file_read_state_from_modifiers(tool_calls.into_iter(), 10);
    assert!(states.is_empty());
}

#[test]
fn test_build_file_state_ignores_non_read_tools() {
    // Non-read tools should be ignored even if they have FileRead modifiers
    let modifiers: Vec<ContextModifier> = vec![ContextModifier::FileRead {
        path: PathBuf::from("/tmp/a.rs"),
        content: "written".to_string(),
        file_mtime_ms: Some(3000),
        offset: None,
        limit: None,
        read_kind: FileReadKind::FullContent,
    }];

    let tool_calls = vec![(ToolName::Write.as_str(), modifiers.as_slice(), 1, true)];

    let states = build_file_read_state_from_modifiers(tool_calls.into_iter(), 10);
    assert!(states.is_empty());
}

#[test]
fn test_build_file_state_ignores_incomplete_calls() {
    // Incomplete tool calls should be ignored
    let modifiers: Vec<ContextModifier> = vec![ContextModifier::FileRead {
        path: PathBuf::from("/tmp/a.rs"),
        content: "content".to_string(),
        file_mtime_ms: Some(1000),
        offset: None,
        limit: None,
        read_kind: FileReadKind::FullContent,
    }];

    let tool_calls = vec![(ToolName::Read.as_str(), modifiers.as_slice(), 1, false)]; // not completed

    let states = build_file_read_state_from_modifiers(tool_calls.into_iter(), 10);
    assert!(states.is_empty());
}

#[test]
fn test_build_file_state_marks_partial_correctly() {
    let modifiers: Vec<ContextModifier> = vec![ContextModifier::FileRead {
        path: PathBuf::from("/tmp/a.rs"),
        content: "partial".to_string(),
        file_mtime_ms: Some(1000),
        offset: Some(10),
        limit: Some(20),
        read_kind: FileReadKind::PartialContent,
    }];

    let tool_calls = vec![(ToolName::Read.as_str(), modifiers.as_slice(), 1, true)];

    let states = build_file_read_state_from_modifiers(tool_calls.into_iter(), 10);
    assert_eq!(states.len(), 1);
    assert!(states[0].1.is_partial());
    assert_eq!(states[0].1.kind, FileReadKind::PartialContent);
}

#[test]
fn test_max_entries_limit() {
    // Test that max_entries limits the number of returned entries
    let mut tool_calls = Vec::new();
    for i in 0..20 {
        let modifiers: Vec<ContextModifier> = vec![ContextModifier::FileRead {
            path: PathBuf::from(format!("/file{i}.txt")),
            content: format!("content {i}"),
            file_mtime_ms: None,
            offset: None,
            limit: None,
            read_kind: FileReadKind::FullContent,
        }];
        tool_calls.push((ToolName::Read.as_str(), modifiers, i, true));
    }

    let states = build_file_read_state_from_modifiers(
        tool_calls
            .iter()
            .map(|(n, m, t, c)| (*n, m.as_slice(), *t, *c)),
        5, // max 5 entries
    );

    assert_eq!(states.len(), 5);
}

#[test]
fn test_merge_same_turn_keeps_base() {
    // When merging entries with the same turn, base is kept (no preference for kind)
    let base = vec![(
        PathBuf::from("/tmp/a.rs"),
        FileReadState::with_content("line1\n".to_string(), None, 2, 1, 1), // partial
    )];
    let incoming = vec![(
        PathBuf::from("/tmp/a.rs"),
        FileReadState::complete_with_turn("line1\nline2\n".to_string(), None, 2), // complete, same turn
    )];

    let merged = merge_file_read_state(base, incoming);
    assert_eq!(merged.len(), 1);
    // Base is kept because incoming has same turn (not newer)
    assert!(merged[0].1.is_partial());
    assert_eq!(merged[0].1.content.as_deref(), Some("line1\n"));
}
