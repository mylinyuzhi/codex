use super::*;

#[test]
fn test_prefetch_state_tracking() {
    let mut state = PrefetchState::new();
    assert!(!state.is_surfaced("foo.md"));
    assert!(!state.is_budget_exhausted());

    state.mark_surfaced("foo.md", 1000);
    assert!(state.is_surfaced("foo.md"));
    assert!(!state.is_surfaced("bar.md"));
}

#[test]
fn test_prefetch_budget_exhaustion() {
    let mut state = PrefetchState::new();
    state.mark_surfaced("big.md", MAX_SESSION_MEMORY_BYTES);
    assert!(state.is_budget_exhausted());
}

#[test]
fn test_parse_selection_response_valid() {
    let response = r#"Here are the relevant memories: ["/path/a.md", "/path/b.md"]"#;
    let paths = parse_selection_response(response);
    assert_eq!(paths, vec!["/path/a.md", "/path/b.md"]);
}

#[test]
fn test_parse_selection_response_empty() {
    let response = "No relevant memories found.";
    let paths = parse_selection_response(response);
    assert!(paths.is_empty());
}

#[test]
fn test_parse_selection_response_json_only() {
    let response = r#"["/a.md"]"#;
    let paths = parse_selection_response(response);
    assert_eq!(paths, vec!["/a.md"]);
}

#[test]
fn test_select_heuristic() {
    let scanned = vec![
        ScannedMemory {
            path: std::path::PathBuf::from("/mem/a.md"),
            frontmatter: None,
            mtime_ms: 100,
            header: String::new(),
            size_bytes: 50,
        },
        ScannedMemory {
            path: std::path::PathBuf::from("/mem/b.md"),
            frontmatter: None,
            mtime_ms: 200,
            header: String::new(),
            size_bytes: 50,
        },
    ];
    let state = PrefetchState::new();
    let selected = select_heuristic(&scanned, &state, 1);
    assert_eq!(selected.len(), 1);
}

#[test]
fn test_selection_prompt_includes_system_prompt() {
    let scanned = vec![ScannedMemory {
        path: std::path::PathBuf::from("/mem/test.md"),
        frontmatter: Some(crate::MemoryFrontmatter {
            name: "test".to_string(),
            description: "a test".to_string(),
            memory_type: crate::MemoryEntryType::User,
        }),
        mtime_ms: 1000,
        header: "[today] test.md".to_string(),
        size_bytes: 50,
    }];
    let state = PrefetchState::new();
    let prompt = build_selection_prompt(&scanned, &state, 5, &[]);
    assert!(prompt.contains("selecting memories"));
    assert!(prompt.contains("Be selective and discerning"));
    assert!(prompt.contains("selected_memories"));
    assert!(prompt.contains("test.md"));
}

#[test]
fn test_selection_prompt_includes_recent_tools() {
    let scanned = vec![];
    let state = PrefetchState::new();
    let tools = vec!["Bash".to_string(), "Read".to_string()];
    let prompt = build_selection_prompt(&scanned, &state, 5, &tools);
    assert!(prompt.contains("Recently-used tools: Bash, Read"));
}

use crate::scan::ScannedMemory;
