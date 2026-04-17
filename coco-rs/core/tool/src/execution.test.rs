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

// ── R7-T19: defense-in-depth strip tests ──
//
// `strip_internal_bash_fields` removes underscore-prefixed fields from
// model-provided Bash input as a safeguard against the model trying
// to set internal-only fields like `_simulatedSedEdit`.

#[test]
fn test_strip_simulated_sed_edit_from_bash_input() {
    let input = serde_json::json!({
        "command": "echo hello",
        "_simulatedSedEdit": {
            "filePath": "/etc/passwd",
            "newContent": "malicious"
        }
    });
    let stripped = strip_internal_bash_fields("Bash", input);
    assert!(stripped.get("command").is_some());
    assert!(
        stripped.get("_simulatedSedEdit").is_none(),
        "internal field must be stripped, got: {stripped:?}"
    );
}

#[test]
fn test_strip_passes_through_normal_bash_input() {
    let input = serde_json::json!({
        "command": "ls -la",
        "timeout": 5000,
        "description": "list files"
    });
    let stripped = strip_internal_bash_fields("Bash", input.clone());
    // Normal fields pass through unchanged.
    assert_eq!(stripped, input);
}

#[test]
fn test_strip_does_not_touch_non_bash_tools() {
    // Read tool input has `file_path` but no underscore-prefixed
    // fields. Even if it DID have one, the stripping is gated on
    // `tool_name == Bash` so other tools are untouched.
    let input = serde_json::json!({
        "file_path": "/tmp/foo.txt",
        "_some_internal": "should stay because not Bash"
    });
    let stripped = strip_internal_bash_fields("Read", input.clone());
    assert_eq!(stripped, input);
}

#[test]
fn test_strip_removes_all_underscore_prefixed_bash_fields() {
    // The convention is "any underscore-prefixed key", not just
    // `_simulatedSedEdit` specifically. Future internal fields
    // following the same convention will be stripped automatically.
    let input = serde_json::json!({
        "command": "echo hi",
        "_simulatedSedEdit": { "filePath": "/x", "newContent": "y" },
        "_secretFlag": true,
        "_anotherInternal": 42
    });
    let stripped = strip_internal_bash_fields("Bash", input);
    let obj = stripped.as_object().unwrap();
    assert_eq!(obj.len(), 1);
    assert!(obj.contains_key("command"));
}

#[test]
fn test_strip_handles_non_object_bash_input() {
    // Defensive: Bash input that's not an object (rare but possible
    // in malformed traffic) is returned unchanged.
    let input = serde_json::json!("not an object");
    let stripped = strip_internal_bash_fields("Bash", input.clone());
    assert_eq!(stripped, input);
}
