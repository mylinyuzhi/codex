use super::*;

#[test]
fn test_parse_memory_index() {
    let content = r#"# Memory Index

- [User Role](user_role.md) — user is a senior engineer
- [Feedback Testing](feedback_testing.md) — prefer integration tests
"#;
    let index = parse_memory_index(content);
    assert_eq!(index.entries.len(), 2);
    assert_eq!(index.entries[0].title, "User Role");
    assert_eq!(index.entries[0].file, "user_role.md");
    assert_eq!(index.entries[1].title, "Feedback Testing");
}

#[test]
fn test_parse_memory_entry() {
    let content = r#"---
name: user_role
description: User is a senior engineer
type: user
---

User has 10 years of experience in Rust.
"#;
    let entry =
        parse_memory_entry(std::path::Path::new("test.md"), content).expect("should parse entry");
    assert_eq!(entry.name, "user_role");
    assert_eq!(entry.description, "User is a senior engineer");
    assert!(matches!(entry.memory_type, MemoryEntryType::User));
    assert!(entry.content.contains("10 years"));
}

#[test]
fn test_parse_feedback_memory() {
    let content = r#"---
name: testing_pref
description: prefer real DB tests
type: feedback
---

Do not mock the database.
"#;
    let entry =
        parse_memory_entry(std::path::Path::new("f.md"), content).expect("should parse entry");
    assert!(matches!(entry.memory_type, MemoryEntryType::Feedback));
}

#[test]
fn test_parse_no_frontmatter() {
    let content = "Just plain text without frontmatter.";
    let entry = parse_memory_entry(std::path::Path::new("f.md"), content);
    assert!(entry.is_none());
}

#[test]
fn test_parse_frontmatter_extracts_fields() {
    let content =
        "---\nname: test_entry\ndescription: A test\ntype: project\n---\n\nBody text here.";
    let (fm, body) = parse_frontmatter(content);
    let fm = fm.expect("frontmatter should be present");
    assert_eq!(fm.name, "test_entry");
    assert_eq!(fm.description, "A test");
    assert_eq!(fm.memory_type, MemoryEntryType::Project);
    assert_eq!(body, "Body text here.");
}

#[test]
fn test_parse_frontmatter_missing() {
    let content = "No frontmatter here.";
    let (fm, body) = parse_frontmatter(content);
    assert!(fm.is_none());
    assert_eq!(body, content);
}

#[test]
fn test_format_entry_as_markdown() {
    let entry = MemoryEntry {
        name: "my_note".to_string(),
        description: "A useful note".to_string(),
        memory_type: MemoryEntryType::Feedback,
        content: "Remember to write tests.".to_string(),
        file_path: PathBuf::from("my_note.md"),
    };
    let md = format_entry_as_markdown(&entry);
    assert!(md.starts_with("---\n"));
    assert!(md.contains("name: my_note"));
    assert!(md.contains("type: feedback"));
    assert!(md.contains("Remember to write tests."));
}

#[test]
fn test_save_and_load_entry() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let mgr = MemoryManager::new(tmp.path());

    let entry = MemoryEntry {
        name: "save_test".to_string(),
        description: "Tests save functionality".to_string(),
        memory_type: MemoryEntryType::User,
        content: "Saved content here.".to_string(),
        file_path: PathBuf::from("save_test.md"),
    };
    mgr.save_entry(&entry).expect("save should succeed");

    let entries = mgr.list_entries().expect("list should succeed");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "save_test");
    assert!(entries[0].content.contains("Saved content here."));
}

#[test]
fn test_delete_entry() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let mgr = MemoryManager::new(tmp.path());

    let entry = MemoryEntry {
        name: "to_delete".to_string(),
        description: "Will be deleted".to_string(),
        memory_type: MemoryEntryType::Reference,
        content: "Temporary.".to_string(),
        file_path: PathBuf::from("to_delete.md"),
    };
    mgr.save_entry(&entry).expect("save should succeed");
    assert_eq!(mgr.list_entries().expect("list").len(), 1);

    mgr.delete_entry("to_delete.md")
        .expect("delete should succeed");
    assert_eq!(mgr.list_entries().expect("list").len(), 0);
}

#[test]
fn test_delete_entry_nonexistent() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let mgr = MemoryManager::new(tmp.path());

    let result = mgr.delete_entry("nonexistent.md");
    assert!(result.is_err());
}

#[test]
fn test_update_index() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let mgr = MemoryManager::new(tmp.path());

    let entry1 = MemoryEntry {
        name: "first".to_string(),
        description: "First entry".to_string(),
        memory_type: MemoryEntryType::User,
        content: "Content one.".to_string(),
        file_path: PathBuf::from("first.md"),
    };
    let entry2 = MemoryEntry {
        name: "second".to_string(),
        description: "Second entry".to_string(),
        memory_type: MemoryEntryType::Feedback,
        content: "Content two.".to_string(),
        file_path: PathBuf::from("second.md"),
    };
    mgr.save_entry(&entry1).expect("save should succeed");
    mgr.save_entry(&entry2).expect("save should succeed");

    mgr.update_index().expect("update_index should succeed");

    let index = mgr.load_index().expect("load_index should succeed");
    assert_eq!(index.entries.len(), 2);

    // Verify the index file was written
    let index_content =
        std::fs::read_to_string(mgr.memory_dir.join("MEMORY.md")).expect("should read MEMORY.md");
    assert!(index_content.starts_with("# Memory Index"));
    assert!(index_content.contains("first.md"));
    assert!(index_content.contains("second.md"));
}
