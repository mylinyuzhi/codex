use pretty_assertions::assert_eq;

use super::*;

// ── Read-only tools ──

#[test]
fn test_read_only_always_allowed() {
    let decision =
        classify_for_auto_mode("Read", &serde_json::json!({}), /*is_read_only*/ true);
    assert_eq!(decision, AutoModeDecision::Allow);
}

#[test]
fn test_read_only_even_unknown_tool() {
    let decision = classify_for_auto_mode(
        "CustomTool",
        &serde_json::json!({}),
        /*is_read_only*/ true,
    );
    assert_eq!(decision, AutoModeDecision::Allow);
}

// ── Write/Edit path checks ──

#[test]
fn test_write_relative_path_allowed() {
    let input = serde_json::json!({"file_path": "src/main.rs"});
    let decision = classify_for_auto_mode("Write", &input, /*is_read_only*/ false);
    assert_eq!(decision, AutoModeDecision::Allow);
}

#[test]
fn test_write_tmp_path_allowed() {
    let input = serde_json::json!({"file_path": "/tmp/test.txt"});
    let decision = classify_for_auto_mode("Write", &input, /*is_read_only*/ false);
    assert_eq!(decision, AutoModeDecision::Allow);
}

#[test]
fn test_write_absolute_path_needs_prompt() {
    let input = serde_json::json!({"file_path": "/etc/passwd"});
    let decision = classify_for_auto_mode("Write", &input, /*is_read_only*/ false);
    assert_eq!(
        decision,
        AutoModeDecision::NeedsPrompt {
            reason: "Write to absolute path".into()
        }
    );
}

#[test]
fn test_edit_absolute_path_needs_prompt() {
    let input = serde_json::json!({"file_path": "/home/user/.bashrc"});
    let decision = classify_for_auto_mode("Edit", &input, /*is_read_only*/ false);
    assert_eq!(
        decision,
        AutoModeDecision::NeedsPrompt {
            reason: "Edit to absolute path".into()
        }
    );
}

#[test]
fn test_edit_relative_path_allowed() {
    let input = serde_json::json!({"file_path": "Cargo.toml"});
    let decision = classify_for_auto_mode("Edit", &input, /*is_read_only*/ false);
    assert_eq!(decision, AutoModeDecision::Allow);
}

#[test]
fn test_write_no_path_needs_prompt() {
    let input = serde_json::json!({"content": "hello"});
    let decision = classify_for_auto_mode("Write", &input, /*is_read_only*/ false);
    assert_eq!(
        decision,
        AutoModeDecision::NeedsPrompt {
            reason: "Write to absolute path".into()
        }
    );
}

// ── Bash commands ──

#[test]
fn test_bash_needs_prompt_by_default() {
    let input = serde_json::json!({"command": "rm -rf /"});
    let decision = classify_for_auto_mode("Bash", &input, /*is_read_only*/ false);
    assert_eq!(
        decision,
        AutoModeDecision::NeedsPrompt {
            reason: "bash command requires review".into()
        }
    );
}

#[test]
fn test_bash_read_only_allowed_via_extended() {
    let decision = classify_auto_mode_extended(&AutoModeInput {
        tool_name: "Bash",
        input: &serde_json::json!({"command": "ls -la"}),
        is_read_only: false,
        bash_is_read_only: true,
    });
    assert_eq!(decision, AutoModeDecision::Allow);
}

#[test]
fn test_bash_non_read_only_needs_prompt_via_extended() {
    let decision = classify_auto_mode_extended(&AutoModeInput {
        tool_name: "Bash",
        input: &serde_json::json!({"command": "rm -rf /"}),
        is_read_only: false,
        bash_is_read_only: false,
    });
    assert_eq!(
        decision,
        AutoModeDecision::NeedsPrompt {
            reason: "bash command requires review".into()
        }
    );
}

// ── Task/Todo tools ──

#[test]
fn test_task_tools_allowed() {
    for tool in &[
        "TaskCreate",
        "TaskUpdate",
        "TaskGet",
        "TaskList",
        "TaskStop",
        "TaskOutput",
        "TodoWrite",
    ] {
        let decision = classify_for_auto_mode(tool, &serde_json::json!({}), false);
        assert_eq!(decision, AutoModeDecision::Allow, "expected {tool} allowed");
    }
}

// ── Plan mode tools ──

#[test]
fn test_plan_mode_tools_allowed() {
    assert_eq!(
        classify_for_auto_mode("EnterPlanMode", &serde_json::json!({}), false),
        AutoModeDecision::Allow
    );
    assert_eq!(
        classify_for_auto_mode("ExitPlanMode", &serde_json::json!({}), false),
        AutoModeDecision::Allow
    );
}

// ── Agent tools need prompt ──

#[test]
fn test_agent_tools_need_prompt() {
    let decision = classify_for_auto_mode("Agent", &serde_json::json!({}), false);
    assert!(matches!(decision, AutoModeDecision::NeedsPrompt { .. }));
}

// ── Web tools need prompt ──

#[test]
fn test_web_tools_need_prompt() {
    let decision = classify_for_auto_mode("WebFetch", &serde_json::json!({}), false);
    assert!(matches!(decision, AutoModeDecision::NeedsPrompt { .. }));
}

// ── MCP tools need prompt ──

#[test]
fn test_mcp_tools_need_prompt() {
    let decision =
        classify_for_auto_mode("mcp__slack__send_message", &serde_json::json!({}), false);
    assert!(matches!(decision, AutoModeDecision::NeedsPrompt { .. }));
}

// ── Unknown tools ──

#[test]
fn test_unknown_tool_needs_prompt() {
    let decision = classify_for_auto_mode(
        "MyCustomTool",
        &serde_json::json!({}),
        /*is_read_only*/ false,
    );
    assert_eq!(
        decision,
        AutoModeDecision::NeedsPrompt {
            reason: "unknown tool: MyCustomTool".into()
        }
    );
}
