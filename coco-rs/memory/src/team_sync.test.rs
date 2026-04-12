use super::*;
use crate::MemoryEntryType;
use std::path::PathBuf;

fn make_entry(name: &str) -> MemoryEntry {
    MemoryEntry {
        name: name.to_string(),
        description: String::new(),
        memory_type: MemoryEntryType::User,
        content: String::new(),
        file_path: PathBuf::from("test.md"),
    }
}

#[test]
fn test_get_missing_memories() {
    let mut state = TeamMemorySyncState::new();
    state.register_agent_memories("agent-1", vec!["mem-a".into()]);

    let all = vec![
        make_entry("mem-a"),
        make_entry("mem-b"),
        make_entry("mem-c"),
    ];
    let missing = state.get_missing_memories("agent-1", &all);
    assert_eq!(missing.len(), 2);
}

#[test]
fn test_new_agent_gets_all() {
    let state = TeamMemorySyncState::new();
    let all = vec![make_entry("m1"), make_entry("m2")];
    let missing = state.get_missing_memories("new-agent", &all);
    assert_eq!(missing.len(), 2);
}
