use super::*;

#[test]
fn test_session_memory_roundtrip_json() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let session_dir = tmp.path().join("session_abc123");

    let mut memory = SessionMemory::new("abc123");
    memory.add_insight(SessionInsight {
        category: InsightCategory::Decision,
        title: "Use async runtime".to_string(),
        content: "Decided to use tokio for the async runtime.".to_string(),
        confidence: 0.95,
    });
    memory.add_insight(SessionInsight {
        category: InsightCategory::Correction,
        title: "Naming convention".to_string(),
        content: "User prefers snake_case for all Rust identifiers.".to_string(),
        confidence: 1.0,
    });
    memory.last_summarized_message_id = Some("msg-42".to_string());

    // Save
    save_session_memory(&session_dir, &memory).expect("save should succeed");

    // Load
    let loaded = load_session_memory(&session_dir)
        .expect("load should succeed")
        .expect("should find saved memory");

    assert_eq!(loaded.session_id, "abc123");
    assert_eq!(loaded.insights.len(), 2);
    assert_eq!(loaded.insights[0].title, "Use async runtime");
    assert_eq!(loaded.insights[1].category, InsightCategory::Correction);
    assert_eq!(loaded.last_summarized_message_id.as_deref(), Some("msg-42"));
}

#[test]
fn test_load_session_memory_missing_returns_none() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let result = load_session_memory(tmp.path()).expect("should not error");
    assert!(result.is_none(), "missing file should return None");
}

#[test]
fn test_session_memory_to_markdown() {
    let mut memory = SessionMemory::new("test");
    memory.add_insight(SessionInsight {
        category: InsightCategory::Decision,
        title: "API design".to_string(),
        content: "Use builder pattern for complex structs.".to_string(),
        confidence: 0.9,
    });
    memory.add_insight(SessionInsight {
        category: InsightCategory::Discovery,
        title: "Serde quirk".to_string(),
        content: "serde_json::from_str requires owned String for lifetime reasons.".to_string(),
        confidence: 0.8,
    });

    let md = memory.to_markdown();
    assert!(md.contains("## Key Decisions"));
    assert!(md.contains("### API design"));
    assert!(md.contains("builder pattern"));
    assert!(md.contains("## Technical Discoveries"));
    assert!(md.contains("### Serde quirk"));
}

#[test]
fn test_session_memory_empty_to_markdown() {
    let memory = SessionMemory::new("empty");
    assert!(memory.to_markdown().is_empty());
    assert!(memory.is_empty());
}

#[test]
fn test_merge_with_project_memory_creates_new_entries() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let mgr = MemoryManager::new(tmp.path());

    let mut memory = SessionMemory::new("sess1");
    memory.add_insight(SessionInsight {
        category: InsightCategory::Decision,
        title: "Database choice".to_string(),
        content: "Use SQLite for local storage.".to_string(),
        confidence: 1.0,
    });

    let affected = merge_with_project_memory(&memory, &mgr).expect("merge should succeed");
    assert_eq!(affected.len(), 1, "should create one new entry");

    let entries = mgr.list_entries().expect("list should succeed");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "Database choice");
    assert!(entries[0].content.contains("SQLite"));
}

#[test]
fn test_merge_with_project_memory_updates_existing() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let mgr = MemoryManager::new(tmp.path());

    // Create an existing entry
    let existing = MemoryEntry {
        name: "Database choice".to_string(),
        description: "Use SQLite for local storage.".to_string(),
        memory_type: MemoryEntryType::Project,
        content: "Use SQLite for local storage.".to_string(),
        file_path: PathBuf::from("database_choice.md"),
    };
    mgr.save_entry(&existing).expect("save should succeed");

    // Merge a session insight that adds to the existing entry
    let mut memory = SessionMemory::new("sess2");
    memory.add_insight(SessionInsight {
        category: InsightCategory::Decision,
        title: "Database choice".to_string(),
        content: "Also considered LanceDB for vector search.".to_string(),
        confidence: 0.85,
    });

    let affected = merge_with_project_memory(&memory, &mgr).expect("merge should succeed");
    assert_eq!(affected.len(), 1);

    let entries = mgr.list_entries().expect("list should succeed");
    assert_eq!(
        entries.len(),
        1,
        "should update existing, not create duplicate"
    );
    assert!(
        entries[0].content.contains("SQLite"),
        "should keep original content"
    );
    assert!(
        entries[0].content.contains("LanceDB"),
        "should append new content"
    );
}

#[test]
fn test_merge_empty_session_memory_is_noop() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let mgr = MemoryManager::new(tmp.path());

    let memory = SessionMemory::new("empty_sess");
    let affected = merge_with_project_memory(&memory, &mgr).expect("merge should succeed");
    assert!(affected.is_empty());
}

#[test]
fn test_parse_session_memory_markdown() {
    let md = r#"## Key Decisions

### API design
Use builder pattern for complex structs.

## Technical Discoveries

### Serde quirk
serde_json requires owned String.
"#;

    let memory = parse_session_memory_markdown("test", md);
    assert_eq!(memory.insights.len(), 2);
    assert_eq!(memory.insights[0].category, InsightCategory::Decision);
    assert_eq!(memory.insights[0].title, "API design");
    assert!(memory.insights[0].content.contains("builder pattern"));
    assert_eq!(memory.insights[1].category, InsightCategory::Discovery);
}

#[test]
fn test_sanitize_filename() {
    assert_eq!(sanitize_filename("Hello World"), "hello_world");
    assert_eq!(sanitize_filename("API-Design_v2"), "api_design_v2");
    assert_eq!(sanitize_filename("  spaces  "), "spaces");
}
