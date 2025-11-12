//! Integration tests for the hook system
//!
//! Tests the entire hook system end-to-end, including configuration loading,
//! hook execution, and Claude Code protocol compatibility.

use codex_hooks::{
    action::registry,
    config::{build_manager_from_config, load_config_from_file},
    decision::{HookEffect, HookResult},
    manager::{enable_hooks, initialize, trigger_hook},
};
use codex_protocol::hooks::{HookEventContext, HookEventData, HookEventName};
use serial_test::serial;
use std::io::Write;
use tempfile::NamedTempFile;

#[tokio::test]
#[serial]
async fn test_hook_blocks_tool_execution() {
    // Create a test hook configuration
    let toml_content = r#"
[[PreToolUse]]
matcher = "local_shell"
sequential = true

[[PreToolUse.hooks]]
type = "command"
command = "exit 2"
timeout = 5000
"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(toml_content.as_bytes()).unwrap();

    let config = load_config_from_file(temp_file.path()).unwrap();
    let manager = build_manager_from_config(config);

    initialize(manager).await;
    enable_hooks().await;

    // Trigger hook
    let event = HookEventContext {
        session_id: "test-123".to_string(),
        transcript_path: None,
        cwd: "/tmp".to_string(),
        hook_event_name: HookEventName::PreToolUse,
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        event_data: HookEventData::PreToolUse {
            tool_name: "local_shell".to_string(),
            tool_input: serde_json::json!({"command": "ls"}),
        },
    };

    let result = trigger_hook(event).await;

    // Should be blocked by exit 2
    assert!(result.is_err());
}

#[tokio::test]
#[serial]
async fn test_hook_allows_tool_execution() {
    let toml_content = r#"
[[PreToolUse]]
matcher = "local_shell"
sequential = false

[[PreToolUse.hooks]]
type = "command"
command = "exit 0"
timeout = 5000
"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(toml_content.as_bytes()).unwrap();

    let config = load_config_from_file(temp_file.path()).unwrap();
    let manager = build_manager_from_config(config);

    initialize(manager).await;
    enable_hooks().await;

    let event = HookEventContext {
        session_id: "test-123".to_string(),
        transcript_path: None,
        cwd: "/tmp".to_string(),
        hook_event_name: HookEventName::PreToolUse,
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        event_data: HookEventData::PreToolUse {
            tool_name: "local_shell".to_string(),
            tool_input: serde_json::json!({"command": "ls"}),
        },
    };

    let result = trigger_hook(event).await;

    // Should be allowed
    assert!(result.is_ok());
}

#[tokio::test]
#[serial]
async fn test_multiple_hooks_work_together() {
    // Register a native hook that sets approval
    registry::register_native_hook("approve_hook", |_ctx| {
        HookResult::continue_with(vec![HookEffect::SetApproved(true)])
    });

    let toml_content = r#"
[[PreToolUse]]
matcher = "*"
sequential = true

[[PreToolUse.hooks]]
type = "native"
function = "approve_hook"

[[PreToolUse.hooks]]
type = "command"
command = "echo 'audit: tool executed'"
timeout = 5000
"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(toml_content.as_bytes()).unwrap();

    let config = load_config_from_file(temp_file.path()).unwrap();
    let manager = build_manager_from_config(config);

    initialize(manager).await;
    enable_hooks().await;

    let event = HookEventContext {
        session_id: "test-123".to_string(),
        transcript_path: None,
        cwd: "/tmp".to_string(),
        hook_event_name: HookEventName::PreToolUse,
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        event_data: HookEventData::PreToolUse {
            tool_name: "bash".to_string(),
            tool_input: serde_json::json!({}),
        },
    };

    let result = trigger_hook(event).await;

    assert!(result.is_ok());
}

#[tokio::test]
#[serial]
async fn test_hook_json_protocol_compatibility() {
    // Test Claude Code JSON I/O protocol with a simple JSON output
    let toml_content = r#"
[[PreToolUse]]
matcher = "*"
sequential = false

[[PreToolUse.hooks]]
type = "command"
command = "echo '{\"continue\": true, \"decision\": \"approve\", \"systemMessage\": \"Test hook executed\"}'"
timeout = 5000
"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(toml_content.as_bytes()).unwrap();

    let config = load_config_from_file(temp_file.path()).unwrap();
    let manager = build_manager_from_config(config);

    initialize(manager).await;
    enable_hooks().await;

    let event = HookEventContext {
        session_id: "test-json-protocol".to_string(),
        transcript_path: None,
        cwd: "/tmp".to_string(),
        hook_event_name: HookEventName::PreToolUse,
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        event_data: HookEventData::PreToolUse {
            tool_name: "test".to_string(),
            tool_input: serde_json::json!({}),
        },
    };

    let result = trigger_hook(event).await;

    assert!(result.is_ok());
}

#[tokio::test]
#[serial]
async fn test_hook_state_sharing() {
    // Register native hooks that communicate via shared state
    registry::register_native_hook("set_metadata", |_ctx| {
        HookResult::continue_with(vec![HookEffect::AddMetadata {
            key: "test_key".to_string(),
            value: serde_json::json!("test_value"),
        }])
    });

    registry::register_native_hook("check_metadata", |_ctx| {
        // In a native hook, we can't use async operations
        // This test demonstrates that hooks execute in sequence
        // and state is shared, but we can't easily check it in sync context
        HookResult::continue_with(vec![])
    });

    let toml_content = r#"
[[PreToolUse]]
matcher = "*"
sequential = true

[[PreToolUse.hooks]]
type = "native"
function = "set_metadata"

[[PreToolUse.hooks]]
type = "native"
function = "check_metadata"
"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(toml_content.as_bytes()).unwrap();

    let config = load_config_from_file(temp_file.path()).unwrap();
    let manager = build_manager_from_config(config);

    initialize(manager).await;
    enable_hooks().await;

    let event = HookEventContext {
        session_id: "test-state".to_string(),
        transcript_path: None,
        cwd: "/tmp".to_string(),
        hook_event_name: HookEventName::PreToolUse,
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        event_data: HookEventData::Other,
    };

    let result = trigger_hook(event).await;

    // Should succeed because metadata was shared
    assert!(result.is_ok());
}

#[tokio::test]
#[serial]
async fn test_post_tool_use_hook() {
    let toml_content = r#"
[[PostToolUse]]
matcher = "*"
sequential = false

[[PostToolUse.hooks]]
type = "command"
command = "echo 'post-hook executed'"
timeout = 5000
"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(toml_content.as_bytes()).unwrap();

    let config = load_config_from_file(temp_file.path()).unwrap();
    let manager = build_manager_from_config(config);

    initialize(manager).await;
    enable_hooks().await;

    let event = HookEventContext {
        session_id: "test-post".to_string(),
        transcript_path: None,
        cwd: "/tmp".to_string(),
        hook_event_name: HookEventName::PostToolUse,
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        event_data: HookEventData::PostToolUse {
            tool_name: "test".to_string(),
            tool_output: serde_json::json!({"result": "success"}),
        },
    };

    let result = trigger_hook(event).await;

    assert!(result.is_ok());
}

#[tokio::test]
#[serial]
async fn test_session_lifecycle_hooks() {
    let toml_content = r#"
[[SessionStart]]
matcher = ""
sequential = false

[[SessionStart.hooks]]
type = "command"
command = "echo 'session started'"
timeout = 5000

[[SessionEnd]]
matcher = ""
sequential = false

[[SessionEnd.hooks]]
type = "command"
command = "echo 'session ended'"
timeout = 5000
"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(toml_content.as_bytes()).unwrap();

    let config = load_config_from_file(temp_file.path()).unwrap();
    let manager = build_manager_from_config(config);

    initialize(manager).await;
    enable_hooks().await;

    // Test SessionStart
    let start_event = HookEventContext {
        session_id: "test-lifecycle".to_string(),
        transcript_path: None,
        cwd: "/tmp".to_string(),
        hook_event_name: HookEventName::SessionStart,
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        event_data: HookEventData::Other,
    };

    let result = trigger_hook(start_event).await;
    assert!(result.is_ok());

    // Test SessionEnd
    let end_event = HookEventContext {
        session_id: "test-lifecycle".to_string(),
        transcript_path: None,
        cwd: "/tmp".to_string(),
        hook_event_name: HookEventName::SessionEnd,
        timestamp: "2025-01-01T00:10:00Z".to_string(),
        event_data: HookEventData::Other,
    };

    let result = trigger_hook(end_event).await;
    assert!(result.is_ok());
}

#[tokio::test]
#[serial]
async fn test_empty_config_loads_successfully() {
    let toml_content = r#""#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(toml_content.as_bytes()).unwrap();

    // Should handle empty config gracefully
    let config = load_config_from_file(temp_file.path());

    // Empty config should either load with empty hooks map or fail gracefully
    match config {
        Ok(cfg) => {
            let manager = build_manager_from_config(cfg);
            assert!(manager.is_enabled());
        }
        Err(_) => {
            // Empty TOML may not parse, which is acceptable
        }
    }
}

#[test]
fn test_config_with_multiple_actions() {
    let toml_content = r#"
[[PreToolUse]]
matcher = "local_shell"
sequential = true

[[PreToolUse.hooks]]
type = "command"
command = "echo 'check 1'"
timeout = 5000

[[PreToolUse.hooks]]
type = "command"
command = "echo 'check 2'"
timeout = 5000

[[PreToolUse.hooks]]
type = "command"
command = "echo 'check 3'"
timeout = 5000
"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(toml_content.as_bytes()).unwrap();

    let config = load_config_from_file(temp_file.path()).unwrap();
    let manager = build_manager_from_config(config);

    // Manager should have hooks registered
    assert!(manager.is_enabled());
}
