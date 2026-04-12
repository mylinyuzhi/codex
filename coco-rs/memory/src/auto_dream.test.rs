use super::*;
use crate::MemoryEntry;
use std::path::PathBuf;

fn make_entry(name: &str, desc: &str, t: MemoryEntryType) -> MemoryEntry {
    MemoryEntry {
        name: name.to_string(),
        description: desc.to_string(),
        memory_type: t,
        content: String::new(),
        file_path: PathBuf::from("test.md"),
    }
}

#[test]
fn test_should_consolidate() {
    let entries = vec![make_entry("a", "desc", MemoryEntryType::User)];
    let config = AutoDreamConfig::default();
    assert!(!should_consolidate(&entries, 1, &config));
    assert!(should_consolidate(&entries, 3, &config));
}

#[test]
fn test_find_merge_candidates() {
    let entries = vec![
        make_entry("foo", "the foo setting", MemoryEntryType::User),
        make_entry("foo", "foo configuration", MemoryEntryType::User),
        make_entry("bar", "something else", MemoryEntryType::Feedback),
    ];
    let candidates = find_merge_candidates(&entries);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0], (0, 1));
}

#[test]
fn test_entries_overlap() {
    let a = make_entry("x", "the user prefers rust code", MemoryEntryType::User);
    let b = make_entry(
        "y",
        "user prefers rust implementations",
        MemoryEntryType::User,
    );
    assert!(entries_overlap(&a, &b));

    let c = make_entry("z", "something completely different", MemoryEntryType::User);
    assert!(!entries_overlap(&a, &c));
}
