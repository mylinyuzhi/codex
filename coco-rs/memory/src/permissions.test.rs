use super::*;

#[test]
fn test_read_tools_allowed() {
    assert_eq!(evaluate_extraction_tool("Read"), ToolPermission::Allow);
    assert_eq!(evaluate_extraction_tool("Grep"), ToolPermission::Allow);
    assert_eq!(evaluate_extraction_tool("Glob"), ToolPermission::Allow);
}

#[test]
fn test_bash_read_only() {
    assert_eq!(
        evaluate_extraction_tool("Bash"),
        ToolPermission::AllowReadOnly
    );
}

#[test]
fn test_write_tools_memdir_only() {
    assert_eq!(
        evaluate_extraction_tool("Write"),
        ToolPermission::AllowIfMemdir
    );
    assert_eq!(
        evaluate_extraction_tool("Edit"),
        ToolPermission::AllowIfMemdir
    );
}

#[test]
fn test_agent_denied() {
    let result = evaluate_extraction_tool("Agent");
    assert!(matches!(result, ToolPermission::Deny { .. }));
}

#[test]
fn test_mcp_denied() {
    let result = evaluate_extraction_tool("mcp__slack_send");
    assert!(matches!(result, ToolPermission::Deny { .. }));
}

#[test]
fn test_unknown_denied() {
    let result = evaluate_extraction_tool("SomeFutureTool");
    assert!(matches!(result, ToolPermission::Deny { .. }));
}

#[test]
fn test_is_read_only_command() {
    assert!(is_read_only_command("ls -la"));
    assert!(is_read_only_command("cat foo.md"));
    assert!(is_read_only_command("grep pattern file"));
    assert!(is_read_only_command("head -10 file"));
    assert!(!is_read_only_command("rm -rf /"));
    assert!(!is_read_only_command("echo hello > file"));
    assert!(!is_read_only_command("sed -i 's/a/b/' file"));
}

#[test]
fn test_is_memdir_path() {
    let mem_dir = std::path::Path::new("/home/.claude/memory");
    assert!(is_memdir_path("/home/.claude/memory/foo.md", mem_dir));
    assert!(!is_memdir_path("/home/project/src/main.rs", mem_dir));
    // Relative path resolved against memory_dir
    assert!(is_memdir_path("foo.md", mem_dir));
}
