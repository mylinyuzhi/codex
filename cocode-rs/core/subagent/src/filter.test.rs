use super::*;
use cocode_protocol::PermissionMode;

fn all_tools() -> Vec<String> {
    vec![
        "Bash".to_string(),
        "Read".to_string(),
        "Edit".to_string(),
        "Write".to_string(),
        "Glob".to_string(),
        "Grep".to_string(),
        "Task".to_string(),
        "EnterPlanMode".to_string(),
        "ExitPlanMode".to_string(),
        "TaskStop".to_string(),
        "AskUserQuestion".to_string(),
        "EnterWorktree".to_string(),
    ]
}

fn make_def(tools: Vec<&str>, disallowed: Vec<&str>) -> AgentDefinition {
    AgentDefinition {
        agent_type: "test".into(),
        name: "test".into(),
        description: "test agent".into(),
        tools: tools.into_iter().map(String::from).collect(),
        disallowed_tools: disallowed.into_iter().map(String::from).collect(),
        ..Default::default()
    }
}

#[test]
fn test_system_blocked_always_removed() {
    let def = make_def(vec![], vec![]);
    let result = filter_tools_for_agent(&all_tools(), &def, false, None);
    assert!(!result.tools.contains(&"Task".to_string()));
    assert!(!result.tools.contains(&"EnterPlanMode".to_string()));
    assert!(!result.tools.contains(&"ExitPlanMode".to_string()));
    assert!(!result.tools.contains(&"TaskStop".to_string()));
    assert!(!result.tools.contains(&"AskUserQuestion".to_string()));
    assert!(!result.tools.contains(&"EnterWorktree".to_string()));
}

#[test]
fn test_allow_list_filtering() {
    let def = make_def(vec!["Bash", "Read"], vec![]);
    let result = filter_tools_for_agent(&all_tools(), &def, false, None);
    assert_eq!(result.tools, vec!["Bash", "Read"]);
}

#[test]
fn test_deny_list_filtering() {
    let def = make_def(vec![], vec!["Edit", "Write"]);
    let result = filter_tools_for_agent(&all_tools(), &def, false, None);
    assert!(result.tools.contains(&"Bash".to_string()));
    assert!(result.tools.contains(&"Read".to_string()));
    assert!(!result.tools.contains(&"Edit".to_string()));
    assert!(!result.tools.contains(&"Write".to_string()));
}

#[test]
fn test_combined_allow_deny() {
    let def = make_def(vec!["Bash", "Read", "Edit"], vec!["Edit"]);
    let result = filter_tools_for_agent(&all_tools(), &def, false, None);
    assert_eq!(result.tools, vec!["Bash", "Read"]);
}

#[test]
fn test_background_limits_to_async_safe() {
    let def = make_def(vec![], vec![]);
    let all = vec![
        "Bash",
        "Read",
        "Edit",
        "Write",
        "Glob",
        "Grep",
        "WebFetch",
        "WebSearch",
        "NotebookEdit",
        "TaskOutput",
        "SomeInteractiveTool",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();
    let result = filter_tools_for_agent(&all, &def, true, None);
    assert!(!result.tools.contains(&"SomeInteractiveTool".to_string()));
    assert!(result.tools.contains(&"Bash".to_string()));
    assert!(result.tools.contains(&"Read".to_string()));
    assert!(result.tools.contains(&"Edit".to_string()));
    assert!(result.tools.contains(&"WebFetch".to_string()));
    assert!(result.tools.contains(&"TaskOutput".to_string()));
}

#[test]
fn test_background_also_applies_system_blocked() {
    let def = make_def(vec![], vec![]);
    let result = filter_tools_for_agent(&all_tools(), &def, true, None);
    // AskUserQuestion is system-blocked for ALL subagents (foreground + background)
    assert!(!result.tools.contains(&"AskUserQuestion".to_string()));
    // Task is system-blocked
    assert!(!result.tools.contains(&"Task".to_string()));
}

#[test]
fn test_ask_user_blocked_for_all_subagents() {
    let def = make_def(vec![], vec![]);
    // AskUserQuestion is blocked even for foreground subagents
    let result = filter_tools_for_agent(&all_tools(), &def, false, None);
    assert!(!result.tools.contains(&"AskUserQuestion".to_string()));
}

#[test]
fn test_empty_tools_in() {
    let def = make_def(vec![], vec![]);
    let result = filter_tools_for_agent(&[], &def, false, None);
    assert!(result.tools.is_empty());
}

// ── Task(type) restriction tests ──

#[test]
fn test_task_restriction_parsing() {
    let types = parse_task_restriction("Task(explore, bash)");
    assert_eq!(types, Some(vec!["explore".to_string(), "bash".to_string()]));
}

#[test]
fn test_task_restriction_single_type() {
    let types = parse_task_restriction("Task(explore)");
    assert_eq!(types, Some(vec!["explore".to_string()]));
}

#[test]
fn test_task_restriction_not_task() {
    assert!(parse_task_restriction("Read").is_none());
    assert!(parse_task_restriction("Task").is_none());
    assert!(parse_task_restriction("Task()").is_none());
}

#[test]
fn test_task_restriction_in_allow_list() {
    let def = make_def(vec!["Read", "Task(explore, bash)"], vec![]);
    let all = vec!["Read".to_string(), "Task".to_string(), "Edit".to_string()];
    let result = filter_tools_for_agent(&all, &def, false, None);
    // Task should be in the filtered tools (normalized from Task(explore, bash))
    assert!(result.tools.contains(&"Task".to_string()));
    assert!(result.tools.contains(&"Read".to_string()));
    assert!(!result.tools.contains(&"Edit".to_string()));
    // Restrictions should be extracted
    assert_eq!(
        result.task_type_restrictions,
        Some(vec!["explore".to_string(), "bash".to_string()])
    );
}

#[test]
fn test_no_task_restriction_when_plain_tools() {
    let def = make_def(vec!["Read", "Task"], vec![]);
    let all = vec!["Read".to_string(), "Task".to_string()];
    let result = filter_tools_for_agent(&all, &def, false, None);
    assert!(result.task_type_restrictions.is_none());
}

#[test]
fn test_normalize_tool_name() {
    assert_eq!(normalize_tool_name("Task(explore)"), "Task");
    assert_eq!(normalize_tool_name("Read"), "Read");
    assert_eq!(normalize_tool_name("Task"), "Task");
}

// ── MCP tool passthrough tests ──

#[test]
fn test_mcp_tools_bypass_all_filtering() {
    // MCP tools should pass through system-blocked, allow-list, deny-list, and background filter
    let def = make_def(vec!["Read"], vec!["mcp__server__tool"]);
    let all = vec![
        "Read".to_string(),
        "mcp__server__tool".to_string(),
        "mcp__other__action".to_string(),
    ];

    // Foreground: MCP tools bypass allow-list and deny-list
    let result = filter_tools_for_agent(&all, &def, false, None);
    assert!(result.tools.contains(&"mcp__server__tool".to_string()));
    assert!(result.tools.contains(&"mcp__other__action".to_string()));
    assert!(result.tools.contains(&"Read".to_string()));

    // Background: MCP tools bypass ASYNC_SAFE_TOOLS filter
    let result = filter_tools_for_agent(&all, &def, true, None);
    assert!(result.tools.contains(&"mcp__server__tool".to_string()));
    assert!(result.tools.contains(&"mcp__other__action".to_string()));
}

#[test]
fn test_mcp_tools_bypass_system_blocked_and_background() {
    let def = make_def(vec![], vec![]);
    let all = vec![
        "Bash".to_string(),
        "Task".to_string(),
        "mcp__myserver__run".to_string(),
    ];
    // Background mode
    let result = filter_tools_for_agent(&all, &def, true, None);
    // Task is system-blocked
    assert!(!result.tools.contains(&"Task".to_string()));
    // mcp__ tool passes through everything
    assert!(result.tools.contains(&"mcp__myserver__run".to_string()));
    assert!(result.tools.contains(&"Bash".to_string()));
}

// ── ExitPlanMode plan mode exception tests ──

#[test]
fn test_exit_plan_mode_allowed_in_plan_permission() {
    let def = make_def(vec![], vec![]);
    let all = vec![
        "Bash".to_string(),
        "ExitPlanMode".to_string(),
        "EnterPlanMode".to_string(),
    ];
    let result = filter_tools_for_agent(&all, &def, false, Some(&PermissionMode::Plan));
    // ExitPlanMode should be allowed in Plan mode
    assert!(result.tools.contains(&"ExitPlanMode".to_string()));
    // EnterPlanMode should still be blocked
    assert!(!result.tools.contains(&"EnterPlanMode".to_string()));
}

#[test]
fn test_exit_plan_mode_blocked_in_default_permission() {
    let def = make_def(vec![], vec![]);
    let all = vec!["Bash".to_string(), "ExitPlanMode".to_string()];
    let result = filter_tools_for_agent(&all, &def, false, Some(&PermissionMode::Default));
    assert!(!result.tools.contains(&"ExitPlanMode".to_string()));
}

#[test]
fn test_exit_plan_mode_blocked_with_no_permission() {
    let def = make_def(vec![], vec![]);
    let all = vec!["Bash".to_string(), "ExitPlanMode".to_string()];
    let result = filter_tools_for_agent(&all, &def, false, None);
    assert!(!result.tools.contains(&"ExitPlanMode".to_string()));
}

// ── ASYNC_SAFE_TOOLS completeness tests ──

#[test]
fn test_async_safe_tools_include_skill_and_mcp_search() {
    let def = make_def(vec![], vec![]);
    // Use exact tool name strings matching ToolName::X.as_str()
    let all = vec![
        ToolName::Skill.as_str().to_string(),
        ToolName::McpSearch.as_str().to_string(),
        ToolName::TodoWrite.as_str().to_string(),
        ToolName::EnterWorktree.as_str().to_string(),
        ToolName::ExitWorktree.as_str().to_string(),
    ];
    let result = filter_tools_for_agent(&all, &def, true, None);
    assert!(result.tools.contains(&ToolName::Skill.as_str().to_string()));
    assert!(
        result
            .tools
            .contains(&ToolName::McpSearch.as_str().to_string())
    );
    assert!(
        result
            .tools
            .contains(&ToolName::TodoWrite.as_str().to_string())
    );
    // EnterWorktree and ExitWorktree are system-blocked (Layer 1),
    // but they are in ASYNC_SAFE_TOOLS for when they pass Layer 1
    // (e.g., when allowed via definition override). In background mode with
    // no allow-list, they are removed by Layer 1 first.
}
