use super::*;

#[test]
fn test_templates_non_empty() {
    assert!(!BASE_IDENTITY.is_empty());
    assert!(!TOOL_POLICY.is_empty());
    assert!(!SECURITY.is_empty());
    assert!(!GIT_WORKFLOW.is_empty());
    assert!(!TASK_MANAGEMENT.is_empty());
    assert!(!MCP_INSTRUCTIONS.is_empty());
    assert!(!ENVIRONMENT_TEMPLATE.is_empty());
    assert!(!PERMISSION_DEFAULT.is_empty());
    assert!(!PERMISSION_PLAN.is_empty());
    assert!(!PERMISSION_ACCEPT_EDITS.is_empty());
    assert!(!PERMISSION_BYPASS.is_empty());
    assert!(!EXPLORE_SUBAGENT.is_empty());
    assert!(!PLAN_SUBAGENT.is_empty());
    assert!(!SUMMARIZATION.is_empty());
}

#[test]
fn test_environment_template_has_placeholders() {
    assert!(ENVIRONMENT_TEMPLATE.contains("{{platform}}"));
    assert!(ENVIRONMENT_TEMPLATE.contains("{{cwd}}"));
    assert!(ENVIRONMENT_TEMPLATE.contains("{{date}}"));
}
