use super::*;

#[test]
fn test_tool_config_default() {
    let config = ToolConfig::default();
    assert_eq!(config.max_tool_concurrency, DEFAULT_MAX_TOOL_CONCURRENCY);
    assert!(config.mcp_tool_timeout.is_none());
    assert_eq!(config.max_result_size, DEFAULT_MAX_RESULT_SIZE);
    assert_eq!(config.result_preview_size, DEFAULT_RESULT_PREVIEW_SIZE);
    assert!(config.enable_result_persistence);
}

#[test]
fn test_tool_config_serde() {
    let json = r#"{"max_tool_concurrency": 5, "mcp_tool_timeout": 30000}"#;
    let config: ToolConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.max_tool_concurrency, 5);
    assert_eq!(config.mcp_tool_timeout, Some(30000));
    // Defaults should apply for unspecified fields
    assert_eq!(config.max_result_size, DEFAULT_MAX_RESULT_SIZE);
    assert_eq!(config.result_preview_size, DEFAULT_RESULT_PREVIEW_SIZE);
    assert!(config.enable_result_persistence);
}

#[test]
fn test_tool_config_serde_defaults() {
    let json = r#"{}"#;
    let config: ToolConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.max_tool_concurrency, DEFAULT_MAX_TOOL_CONCURRENCY);
    assert!(config.mcp_tool_timeout.is_none());
    assert_eq!(config.max_result_size, DEFAULT_MAX_RESULT_SIZE);
    assert_eq!(config.result_preview_size, DEFAULT_RESULT_PREVIEW_SIZE);
    assert!(config.enable_result_persistence);
}

#[test]
fn test_tool_config_persistence_disabled() {
    let json = r#"{"enable_result_persistence": false}"#;
    let config: ToolConfig = serde_json::from_str(json).unwrap();
    assert!(!config.enable_result_persistence);
}

#[test]
fn test_tool_config_custom_sizes() {
    let json = r#"{"max_result_size": 200000, "result_preview_size": 1000}"#;
    let config: ToolConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.max_result_size, 200_000);
    assert_eq!(config.result_preview_size, 1_000);
}

/// Verify constants match Claude Code v2.1.7 alignment.
#[test]
fn test_claude_code_v217_alignment() {
    assert_eq!(DEFAULT_MAX_RESULT_SIZE, 400_000);
    assert_eq!(DEFAULT_RESULT_PREVIEW_SIZE, 2_000);
    assert_eq!(DEFAULT_MAX_TOOL_CONCURRENCY, 10);
}

#[test]
fn test_apply_patch_tool_type_serde() {
    // Function
    let json = r#""function""#;
    let t: ApplyPatchToolType = serde_json::from_str(json).unwrap();
    assert_eq!(t, ApplyPatchToolType::Function);

    // Freeform
    let json = r#""freeform""#;
    let t: ApplyPatchToolType = serde_json::from_str(json).unwrap();
    assert_eq!(t, ApplyPatchToolType::Freeform);

    // Shell
    let json = r#""shell""#;
    let t: ApplyPatchToolType = serde_json::from_str(json).unwrap();
    assert_eq!(t, ApplyPatchToolType::Shell);
}

#[test]
 fn test_apply_patch_tool_type_default() {
    assert_eq!(ApplyPatchToolType::default(), ApplyPatchToolType::Freeform);
}
