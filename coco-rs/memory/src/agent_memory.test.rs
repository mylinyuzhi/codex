use super::*;

use coco_types::MemoryScope;
use std::path::PathBuf;

#[test]
fn test_sanitize_agent_type_for_path() {
    assert_eq!(sanitize_agent_type_for_path("Explore"), "Explore");
    assert_eq!(
        sanitize_agent_type_for_path("my-plugin:my-agent"),
        "my-plugin-my-agent"
    );
    assert_eq!(
        sanitize_agent_type_for_path("a:b:c"),
        "a-b-c",
        "every colon, not just the first, must be replaced",
    );
}

#[test]
fn test_agent_memory_dir_per_scope() {
    let cwd = PathBuf::from("/work/proj");
    let home = PathBuf::from("/home/me");

    assert_eq!(
        agent_memory_dir("Explore", MemoryScope::User, &cwd, &home),
        PathBuf::from("/home/me/.coco/agent-memory/Explore"),
    );
    assert_eq!(
        agent_memory_dir("Explore", MemoryScope::Project, &cwd, &home),
        PathBuf::from("/work/proj/.coco/agent-memory/Explore"),
    );
    assert_eq!(
        agent_memory_dir("Explore", MemoryScope::Local, &cwd, &home),
        PathBuf::from("/work/proj/.coco/agent-memory-local/Explore"),
    );
}

#[test]
fn test_agent_memory_entrypoint_appends_memory_md() {
    let cwd = PathBuf::from("/work/proj");
    let home = PathBuf::from("/home/me");

    assert_eq!(
        agent_memory_entrypoint("Plan", MemoryScope::Project, &cwd, &home),
        PathBuf::from("/work/proj/.coco/agent-memory/Plan/MEMORY.md"),
    );
}

#[test]
fn test_load_agent_memory_prompt_empty_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cwd = tmp.path().to_path_buf();
    let home = tmp.path().join("home");

    // No MEMORY.md exists — fallback empty body.
    let prompt = load_agent_memory_prompt("Explore", MemoryScope::Project, &cwd, &home);
    assert!(prompt.contains("# Persistent Agent Memory"));
    assert!(prompt.contains("MEMORY.md is currently empty"));
}

#[test]
fn test_load_agent_memory_prompt_with_body() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cwd = tmp.path().to_path_buf();
    let home = tmp.path().join("home");
    let dir = agent_memory_dir("bug-hunter", MemoryScope::Project, &cwd, &home);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("MEMORY.md"), "- known: flaky test in foo.rs").unwrap();

    let prompt = load_agent_memory_prompt("bug-hunter", MemoryScope::Project, &cwd, &home);
    assert!(prompt.contains("# Persistent Agent Memory"));
    assert!(prompt.contains("known: flaky test in foo.rs"));
    assert!(!prompt.contains("MEMORY.md is currently empty"));
}

#[test]
fn test_scope_note_per_scope() {
    assert!(scope_note(MemoryScope::User).contains("user-scope"));
    assert!(scope_note(MemoryScope::Project).contains("project-scope"));
    assert!(scope_note(MemoryScope::Local).contains("local-scope"));
}

#[test]
fn test_plugin_namespaced_agent_uses_dash() {
    let cwd = PathBuf::from("/w");
    let home = PathBuf::from("/h");
    let dir = agent_memory_dir("plugin:agent", MemoryScope::User, &cwd, &home);
    assert_eq!(dir, PathBuf::from("/h/.coco/agent-memory/plugin-agent"));
}
