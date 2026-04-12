use super::*;
use std::path::Path;

#[test]
fn test_is_auto_mem_file() {
    let mem_dir = Path::new("/home/user/.claude/memory");
    assert!(is_auto_mem_file(
        Path::new("/home/user/.claude/memory/user_role.md"),
        mem_dir,
    ));
    assert!(!is_auto_mem_file(
        Path::new("/home/user/.claude/other.md"),
        mem_dir,
    ));
}

#[test]
fn test_is_auto_managed_memory_file() {
    let mem_dir = Path::new("/home/user/.claude/memory");
    assert!(is_auto_managed_memory_file(
        Path::new("/home/user/.claude/memory/team/policy.md"),
        mem_dir,
    ));
    assert!(is_auto_managed_memory_file(
        Path::new("/sessions/abc/session_memory.json"),
        mem_dir,
    ));
}

#[test]
fn test_memory_scope() {
    let mem_dir = Path::new("/home/user/.claude/memory");
    assert_eq!(
        memory_scope_for_path(Path::new("/home/user/.claude/memory/user.md"), mem_dir),
        MemoryScope::Personal,
    );
    assert_eq!(
        memory_scope_for_path(Path::new("/home/user/.claude/memory/team/ref.md"), mem_dir),
        MemoryScope::Team,
    );
}

#[test]
fn test_is_memory_directory() {
    assert!(is_memory_directory(Path::new("/home/.claude/memory")));
    assert!(!is_memory_directory(Path::new("/home/.claude/sessions")));
}

#[test]
fn test_detect_session_file_type() {
    assert_eq!(
        detect_session_file_type(Path::new("session_memory.json")),
        SessionFileType::Memory,
    );
    assert_eq!(
        detect_session_file_type(Path::new("transcript.jsonl")),
        SessionFileType::Transcript,
    );
    assert_eq!(
        detect_session_file_type(Path::new("other.txt")),
        SessionFileType::Other,
    );
}

#[test]
fn test_should_bypass_dangerous_dirs() {
    let mem_dir = Path::new("/home/user/.claude/memory");
    // Should bypass when path is in memdir and no override
    assert!(should_bypass_dangerous_dirs(
        Path::new("/home/user/.claude/memory/foo.md"),
        mem_dir,
        /*has_path_override*/ false,
    ));
    // Should NOT bypass when path override is active
    assert!(!should_bypass_dangerous_dirs(
        Path::new("/home/user/.claude/memory/foo.md"),
        mem_dir,
        /*has_path_override*/ true,
    ));
}
