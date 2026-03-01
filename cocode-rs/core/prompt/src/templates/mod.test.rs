use super::*;
use crate::engine;

#[test]
fn test_static_templates_non_empty() {
    assert!(!BASE_IDENTITY.is_empty());
    assert!(!SECURITY.is_empty());
    assert!(!GIT_WORKFLOW.is_empty());
    assert!(!TASK_MANAGEMENT.is_empty());
    assert!(!MCP_INSTRUCTIONS.is_empty());
    assert!(!PERMISSION_DEFAULT.is_empty());
    assert!(!PERMISSION_PLAN.is_empty());
    assert!(!PERMISSION_ACCEPT_EDITS.is_empty());
    assert!(!PERMISSION_BYPASS.is_empty());
    assert!(!SUMMARIZATION.is_empty());
}

#[test]
fn test_rendered_templates_non_empty() {
    // Templates now rendered via minijinja
    assert!(!engine::render("tool_policy", minijinja::context! {}).is_empty());
    assert!(!engine::render("explore_subagent", minijinja::context! {}).is_empty());
    assert!(!engine::render("plan_subagent", minijinja::context! {}).is_empty());
}

#[test]
fn test_tool_names_in_templates() {
    // Verify tool name constants are correctly substituted
    let tool_policy = engine::render("tool_policy", minijinja::context! {});
    assert!(tool_policy.contains(cocode_protocol::ToolName::Bash.as_str()));
    assert!(tool_policy.contains(cocode_protocol::ToolName::Read.as_str()));

    let explore = engine::render("explore_subagent", minijinja::context! {});
    assert!(explore.contains(cocode_protocol::ToolName::Glob.as_str()));
    assert!(explore.contains(cocode_protocol::ToolName::Grep.as_str()));
    assert!(explore.contains(cocode_protocol::ToolName::Read.as_str()));
}
