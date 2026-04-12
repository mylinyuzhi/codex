use super::*;
use crate::config::MemoryConfig;
use std::path::PathBuf;

fn test_hook() -> ExtractionHook {
    init_extraction_hook(PathBuf::from("/tmp/test-memory"), MemoryConfig::default())
}

#[test]
fn test_should_extract_default() {
    let hook = test_hook();
    assert!(should_extract(&hook, /*has_memory_writes*/ false));
}

#[test]
fn test_should_not_extract_when_disabled() {
    let hook = init_extraction_hook(PathBuf::from("/tmp"), MemoryConfig::disabled());
    assert!(!should_extract(&hook, false));
}

#[test]
fn test_should_not_extract_when_writes_detected() {
    let hook = test_hook();
    assert!(!should_extract(&hook, /*has_memory_writes*/ true));
}

#[test]
fn test_should_not_extract_when_in_progress_stashes_trailing() {
    let hook = test_hook();
    begin_extraction(&hook, "msg-1");

    // While in progress, should return false but stash trailing run
    assert!(!should_extract(&hook, false));

    // End extraction should indicate trailing run pending
    let trailing = end_extraction(&hook);
    assert!(trailing);

    // After end, should extract normally
    assert!(should_extract(&hook, false));
}

#[test]
fn test_end_extraction_no_trailing() {
    let hook = test_hook();
    begin_extraction(&hook, "msg-1");
    // No intervening should_extract call → no trailing run
    let trailing = end_extraction(&hook);
    assert!(!trailing);
}

#[test]
fn test_throttle_gate() {
    let hook = init_extraction_hook(
        PathBuf::from("/tmp"),
        MemoryConfig {
            extraction_throttle: 3,
            ..MemoryConfig::default()
        },
    );

    // First call: turns_since=1, throttle=3 → skip
    assert!(!should_extract(&hook, false));

    // Second call: turns_since=2, throttle=3 → skip
    assert!(!should_extract(&hook, false));

    // Third call: turns_since=3 >= throttle=3 → extract, reset to 0
    assert!(should_extract(&hook, false));

    // Fourth call: turns_since=1 again → skip
    assert!(!should_extract(&hook, false));
}

#[test]
fn test_message_count_tracking() {
    let hook = test_hook();
    set_new_message_count(&hook, 42);
    assert_eq!(get_new_message_count(&hook), 42);
}

#[test]
fn test_is_memory_write() {
    let mem_dir = std::path::Path::new("/home/.claude/memory");
    assert!(is_memory_write(
        std::path::Path::new("/home/.claude/memory/foo.md"),
        mem_dir,
    ));
    assert!(!is_memory_write(
        std::path::Path::new("/home/project/src/main.rs"),
        mem_dir,
    ));
}

#[test]
fn test_extraction_context() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("test.md"),
        "---\nname: test\ndescription: test\ntype: user\n---\ncontent",
    )
    .unwrap();

    let ctx = build_extraction_context(dir.path());
    assert_eq!(ctx.file_count, 1);
    assert!(ctx.manifest.contains("test"));
}

#[test]
fn test_get_last_cursor() {
    let hook = test_hook();
    assert!(get_last_cursor(&hook).is_none());
    begin_extraction(&hook, "msg-42");
    assert_eq!(get_last_cursor(&hook).as_deref(), Some("msg-42"));
}
