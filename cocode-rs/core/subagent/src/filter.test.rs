use super::*;

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
        name: "test".to_string(),
        description: "test agent".to_string(),
        agent_type: "test".to_string(),
        tools: tools.into_iter().map(String::from).collect(),
        disallowed_tools: disallowed.into_iter().map(String::from).collect(),
        identity: None,
        max_turns: None,
        permission_mode: None,
        fork_context: false,
        color: None,
        critical_reminder: None,
        source: crate::definition::AgentSource::BuiltIn,
        skills: vec![],
        background: false,
        memory: None,
        hooks: None,
        mcp_servers: None,
        isolation: None,
        use_custom_prompt: false,
    }
}

#[test]
fn test_system_blocked_always_removed() {
    let def = make_def(vec![], vec![]);
    let result = filter_tools_for_agent(&all_tools(), &def, false);
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
    let result = filter_tools_for_agent(&all_tools(), &def, false);
    assert_eq!(result.tools, vec!["Bash", "Read"]);
}

#[test]
fn test_deny_list_filtering() {
    let def = make_def(vec![], vec!["Edit", "Write"]);
    let result = filter_tools_for_agent(&all_tools(), &def, false);
    assert!(result.tools.contains(&"Bash".to_string()));
    assert!(result.tools.contains(&"Read".to_string()));
    assert!(!result.tools.contains(&"Edit".to_string()));
    assert!(!result.tools.contains(&"Write".to_string()));
}

#[test]
fn test_combined_allow_deny() {
    let def = make_def(vec!["Bash", "Read", "Edit"], vec!["Edit"]);
    let result = filter_tools_for_agent(&all_tools(), &def, false);
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
    let result = filter_tools_for_agent(&all, &def, true);
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
    let result = filter_tools_for_agent(&all_tools(), &def, true);
    // AskUserQuestion is system-blocked for ALL subagents (foreground + background)
    assert!(!result.tools.contains(&"AskUserQuestion".to_string()));
    // Task is system-blocked
    assert!(!result.tools.contains(&"Task".to_string()));
}

#[test]
fn test_ask_user_blocked_for_all_subagents() {
    let def = make_def(vec![], vec![]);
    // AskUserQuestion is blocked even for foreground subagents
    let result = filter_tools_for_agent(&all_tools(), &def, false);
    assert!(!result.tools.contains(&"AskUserQuestion".to_string()));
}

#[test]
fn test_empty_tools_in() {
    let def = make_def(vec![], vec![]);
    let result = filter_tools_for_agent(&[], &def, false);
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
    let result = filter_tools_for_agent(&all, &def, false);
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
    let result = filter_tools_for_agent(&all, &def, false);
    assert!(result.task_type_restrictions.is_none());
}

#[test]
fn test_normalize_tool_name() {
    assert_eq!(normalize_tool_name("Task(explore)"), "Task");
    assert_eq!(normalize_tool_name("Read"), "Read");
    assert_eq!(normalize_tool_name("Task"), "Task");
}
