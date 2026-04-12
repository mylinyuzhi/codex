use super::*;
use crate::MemoryEntry;
use crate::MemoryEntryType;
use crate::format_entry_as_markdown;

#[test]
fn test_scan_empty_dir() {
    let dir = tempfile::tempdir().unwrap();
    let files = scan_memory_files(dir.path());
    assert!(files.is_empty());
}

#[test]
fn test_scan_finds_md_files() {
    let dir = tempfile::tempdir().unwrap();
    let entry = MemoryEntry {
        name: "test".to_string(),
        description: "a test memory".to_string(),
        memory_type: MemoryEntryType::User,
        content: "Hello".to_string(),
        file_path: dir.path().join("test.md"),
    };
    let md = format_entry_as_markdown(&entry);
    std::fs::write(dir.path().join("test.md"), &md).unwrap();
    // MEMORY.md should be excluded
    std::fs::write(dir.path().join("MEMORY.md"), "# Index").unwrap();

    let files = scan_memory_files(dir.path());
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].frontmatter.as_ref().unwrap().name, "test");
    assert!(files[0].mtime_ms > 0);
}

#[test]
fn test_scan_sorted_by_mtime() {
    let dir = tempfile::tempdir().unwrap();

    // Write two files with a small time gap
    std::fs::write(
        dir.path().join("old.md"),
        "---\nname: old\ndescription: old\ntype: user\n---\nold",
    )
    .unwrap();

    // Ensure second file has different mtime
    std::thread::sleep(std::time::Duration::from_millis(50));

    std::fs::write(
        dir.path().join("new.md"),
        "---\nname: new\ndescription: new\ntype: user\n---\nnew",
    )
    .unwrap();

    let files = scan_memory_files(dir.path());
    assert_eq!(files.len(), 2);
    // Newest first
    assert_eq!(files[0].frontmatter.as_ref().unwrap().name, "new");
    assert_eq!(files[1].frontmatter.as_ref().unwrap().name, "old");
}

#[test]
fn test_format_manifest() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("user_role.md"),
        "---\nname: user_role\ndescription: Senior engineer\ntype: user\n---\nDetails",
    )
    .unwrap();

    let files = scan_memory_files(dir.path());
    let manifest = format_memory_manifest(&files);
    assert!(manifest.contains("user_role"));
    assert!(manifest.contains("Senior engineer"));
    assert!(manifest.contains("user"));
}
