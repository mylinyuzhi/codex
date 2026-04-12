use super::*;

#[test]
fn test_classify_tool_error() {
    assert_eq!(classify_tool_error(&ToolError::Cancelled), "cancelled");
    assert_eq!(
        classify_tool_error(&ToolError::Timeout { timeout_ms: 1000 }),
        "timeout"
    );
    assert_eq!(
        classify_tool_error(&ToolError::PermissionDenied {
            message: "denied".into()
        }),
        "permission_denied"
    );
}

#[test]
fn test_is_code_editing_tool() {
    assert!(is_code_editing_tool("Edit"));
    assert!(is_code_editing_tool("Write"));
    assert!(!is_code_editing_tool("Read"));
    assert!(!is_code_editing_tool("Glob"));
}

#[test]
fn test_extract_file_extension() {
    assert_eq!(
        extract_file_extension("Read", &serde_json::json!({"file_path": "src/main.rs"})),
        Some("rs".to_string())
    );
    assert_eq!(
        extract_file_extension("Bash", &serde_json::json!({"command": "ls"})),
        None
    );
}

#[test]
fn test_is_deferred_tool() {
    assert!(is_deferred_tool("CronCreate"));
    assert!(is_deferred_tool("NotebookEdit"));
    assert!(!is_deferred_tool("Read"));
    assert!(!is_deferred_tool("Bash"));
}
