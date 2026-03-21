use super::*;
use cocode_protocol::PermissionMode;

fn all_tools() -> Vec<String> {
    vec![
        ToolName::Bash.as_str().to_string(),
        ToolName::Read.as_str().to_string(),
        ToolName::Edit.as_str().to_string(),
        ToolName::Write.as_str().to_string(),
        ToolName::Glob.as_str().to_string(),
        ToolName::Grep.as_str().to_string(),
        ToolName::Task.as_str().to_string(),
        ToolName::EnterPlanMode.as_str().to_string(),
        ToolName::ExitPlanMode.as_str().to_string(),
        ToolName::TaskStop.as_str().to_string(),
        ToolName::AskUserQuestion.as_str().to_string(),
        ToolName::EnterWorktree.as_str().to_string(),
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
    assert!(!result.tools.contains(&ToolName::Task.as_str().to_string()));
    assert!(
        !result
            .tools
            .contains(&ToolName::EnterPlanMode.as_str().to_string())
    );
    assert!(
        !result
            .tools
            .contains(&ToolName::ExitPlanMode.as_str().to_string())
    );
    assert!(
        !result
            .tools
            .contains(&ToolName::TaskStop.as_str().to_string())
    );
    assert!(
        !result
            .tools
            .contains(&ToolName::AskUserQuestion.as_str().to_string())
    );
    assert!(
        !result
            .tools
            .contains(&ToolName::EnterWorktree.as_str().to_string())
    );
}

#[test]
fn test_allow_list_filtering() {
    let def = make_def(
        vec![ToolName::Bash.as_str(), ToolName::Read.as_str()],
        vec![],
    );
    let result = filter_tools_for_agent(&all_tools(), &def, false, None);
    assert_eq!(
        result.tools,
        vec![ToolName::Bash.as_str(), ToolName::Read.as_str()]
    );
}

#[test]
fn test_deny_list_filtering() {
    let def = make_def(
        vec![],
        vec![ToolName::Edit.as_str(), ToolName::Write.as_str()],
    );
    let result = filter_tools_for_agent(&all_tools(), &def, false, None);
    assert!(result.tools.contains(&ToolName::Bash.as_str().to_string()));
    assert!(result.tools.contains(&ToolName::Read.as_str().to_string()));
    assert!(!result.tools.contains(&ToolName::Edit.as_str().to_string()));
    assert!(!result.tools.contains(&ToolName::Write.as_str().to_string()));
}

#[test]
fn test_combined_allow_deny() {
    let def = make_def(
        vec![
            ToolName::Bash.as_str(),
            ToolName::Read.as_str(),
            ToolName::Edit.as_str(),
        ],
        vec![ToolName::Edit.as_str()],
    );
    let result = filter_tools_for_agent(&all_tools(), &def, false, None);
    assert_eq!(
        result.tools,
        vec![ToolName::Bash.as_str(), ToolName::Read.as_str()]
    );
}

#[test]
fn test_background_limits_to_async_safe() {
    let def = make_def(vec![], vec![]);
    let all = vec![
        ToolName::Bash.as_str(),
        ToolName::Read.as_str(),
        ToolName::Edit.as_str(),
        ToolName::Write.as_str(),
        ToolName::Glob.as_str(),
        ToolName::Grep.as_str(),
        ToolName::WebFetch.as_str(),
        ToolName::WebSearch.as_str(),
        ToolName::NotebookEdit.as_str(),
        ToolName::TaskOutput.as_str(),
        "SomeInteractiveTool",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();
    let result = filter_tools_for_agent(&all, &def, true, None);
    assert!(!result.tools.contains(&"SomeInteractiveTool".to_string()));
    assert!(result.tools.contains(&ToolName::Bash.as_str().to_string()));
    assert!(result.tools.contains(&ToolName::Read.as_str().to_string()));
    assert!(result.tools.contains(&ToolName::Edit.as_str().to_string()));
    assert!(
        result
            .tools
            .contains(&ToolName::WebFetch.as_str().to_string())
    );
    assert!(
        result
            .tools
            .contains(&ToolName::TaskOutput.as_str().to_string())
    );
}

#[test]
fn test_background_also_applies_system_blocked() {
    let def = make_def(vec![], vec![]);
    let result = filter_tools_for_agent(&all_tools(), &def, true, None);
    // AskUserQuestion is system-blocked for ALL subagents (foreground + background)
    assert!(
        !result
            .tools
            .contains(&ToolName::AskUserQuestion.as_str().to_string())
    );
    // Task is system-blocked
    assert!(!result.tools.contains(&ToolName::Task.as_str().to_string()));
}

#[test]
fn test_ask_user_blocked_for_all_subagents() {
    let def = make_def(vec![], vec![]);
    // AskUserQuestion is blocked even for foreground subagents
    let result = filter_tools_for_agent(&all_tools(), &def, false, None);
    assert!(
        !result
            .tools
            .contains(&ToolName::AskUserQuestion.as_str().to_string())
    );
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
    assert!(parse_task_restriction(ToolName::Read.as_str()).is_none());
    assert!(parse_task_restriction(ToolName::Task.as_str()).is_none());
    assert!(parse_task_restriction("Task()").is_none());
}

#[test]
fn test_task_restriction_in_allow_list() {
    let def = make_def(vec![ToolName::Read.as_str(), "Task(explore, bash)"], vec![]);
    let all = vec![
        ToolName::Read.as_str().to_string(),
        ToolName::Task.as_str().to_string(),
        ToolName::Edit.as_str().to_string(),
    ];
    let result = filter_tools_for_agent(&all, &def, false, None);
    // Task should be in the filtered tools (normalized from Task(explore, bash))
    assert!(result.tools.contains(&ToolName::Task.as_str().to_string()));
    assert!(result.tools.contains(&ToolName::Read.as_str().to_string()));
    assert!(!result.tools.contains(&ToolName::Edit.as_str().to_string()));
    // Restrictions should be extracted
    assert_eq!(
        result.task_type_restrictions,
        Some(vec!["explore".to_string(), "bash".to_string()])
    );
}

#[test]
fn test_no_task_restriction_when_plain_tools() {
    let def = make_def(
        vec![ToolName::Read.as_str(), ToolName::Task.as_str()],
        vec![],
    );
    let all = vec![
        ToolName::Read.as_str().to_string(),
        ToolName::Task.as_str().to_string(),
    ];
    let result = filter_tools_for_agent(&all, &def, false, None);
    assert!(result.task_type_restrictions.is_none());
}

#[test]
fn test_normalize_tool_name() {
    assert_eq!(
        normalize_tool_name("Task(explore)"),
        ToolName::Task.as_str()
    );
    assert_eq!(
        normalize_tool_name(ToolName::Read.as_str()),
        ToolName::Read.as_str()
    );
    assert_eq!(
        normalize_tool_name(ToolName::Task.as_str()),
        ToolName::Task.as_str()
    );
}

// ── MCP tool passthrough tests ──

#[test]
fn test_mcp_tools_bypass_all_filtering() {
    // MCP tools should pass through system-blocked, allow-list, deny-list, and background filter
    let def = make_def(vec![ToolName::Read.as_str()], vec!["mcp__server__tool"]);
    let all = vec![
        ToolName::Read.as_str().to_string(),
        "mcp__server__tool".to_string(),
        "mcp__other__action".to_string(),
    ];

    // Foreground: MCP tools bypass allow-list and deny-list
    let result = filter_tools_for_agent(&all, &def, false, None);
    assert!(result.tools.contains(&"mcp__server__tool".to_string()));
    assert!(result.tools.contains(&"mcp__other__action".to_string()));
    assert!(result.tools.contains(&ToolName::Read.as_str().to_string()));

    // Background: MCP tools bypass ASYNC_SAFE_TOOLS filter
    let result = filter_tools_for_agent(&all, &def, true, None);
    assert!(result.tools.contains(&"mcp__server__tool".to_string()));
    assert!(result.tools.contains(&"mcp__other__action".to_string()));
}

#[test]
fn test_mcp_tools_bypass_system_blocked_and_background() {
    let def = make_def(vec![], vec![]);
    let all = vec![
        ToolName::Bash.as_str().to_string(),
        ToolName::Task.as_str().to_string(),
        "mcp__myserver__run".to_string(),
    ];
    // Background mode
    let result = filter_tools_for_agent(&all, &def, true, None);
    // Task is system-blocked
    assert!(!result.tools.contains(&ToolName::Task.as_str().to_string()));
    // mcp__ tool passes through everything
    assert!(result.tools.contains(&"mcp__myserver__run".to_string()));
    assert!(result.tools.contains(&ToolName::Bash.as_str().to_string()));
}

// ── ExitPlanMode plan mode exception tests ──

#[test]
fn test_exit_plan_mode_allowed_in_plan_permission() {
    let def = make_def(vec![], vec![]);
    let all = vec![
        ToolName::Bash.as_str().to_string(),
        ToolName::ExitPlanMode.as_str().to_string(),
        ToolName::EnterPlanMode.as_str().to_string(),
    ];
    let result = filter_tools_for_agent(&all, &def, false, Some(&PermissionMode::Plan));
    // ExitPlanMode should be allowed in Plan mode
    assert!(
        result
            .tools
            .contains(&ToolName::ExitPlanMode.as_str().to_string())
    );
    // EnterPlanMode should still be blocked
    assert!(
        !result
            .tools
            .contains(&ToolName::EnterPlanMode.as_str().to_string())
    );
}

#[test]
fn test_exit_plan_mode_blocked_in_default_permission() {
    let def = make_def(vec![], vec![]);
    let all = vec![
        ToolName::Bash.as_str().to_string(),
        ToolName::ExitPlanMode.as_str().to_string(),
    ];
    let result = filter_tools_for_agent(&all, &def, false, Some(&PermissionMode::Default));
    assert!(
        !result
            .tools
            .contains(&ToolName::ExitPlanMode.as_str().to_string())
    );
}

#[test]
fn test_exit_plan_mode_blocked_with_no_permission() {
    let def = make_def(vec![], vec![]);
    let all = vec![
        ToolName::Bash.as_str().to_string(),
        ToolName::ExitPlanMode.as_str().to_string(),
    ];
    let result = filter_tools_for_agent(&all, &def, false, None);
    assert!(
        !result
            .tools
            .contains(&ToolName::ExitPlanMode.as_str().to_string())
    );
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
