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
    }
}

#[test]
fn test_system_blocked_always_removed() {
    let def = make_def(vec![], vec![]);
    let filtered = filter_tools_for_agent(&all_tools(), &def, false);
    assert!(!filtered.contains(&"Task".to_string()));
    assert!(!filtered.contains(&"EnterPlanMode".to_string()));
    assert!(!filtered.contains(&"ExitPlanMode".to_string()));
    assert!(!filtered.contains(&"TaskStop".to_string()));
    assert!(!filtered.contains(&"AskUserQuestion".to_string()));
}

#[test]
fn test_allow_list_filtering() {
    let def = make_def(vec!["Bash", "Read"], vec![]);
    let filtered = filter_tools_for_agent(&all_tools(), &def, false);
    assert_eq!(filtered, vec!["Bash", "Read"]);
}

#[test]
fn test_deny_list_filtering() {
    let def = make_def(vec![], vec!["Edit", "Write"]);
    let filtered = filter_tools_for_agent(&all_tools(), &def, false);
    assert!(filtered.contains(&"Bash".to_string()));
    assert!(filtered.contains(&"Read".to_string()));
    assert!(!filtered.contains(&"Edit".to_string()));
    assert!(!filtered.contains(&"Write".to_string()));
}

#[test]
fn test_combined_allow_deny() {
    let def = make_def(vec!["Bash", "Read", "Edit"], vec!["Edit"]);
    let filtered = filter_tools_for_agent(&all_tools(), &def, false);
    assert_eq!(filtered, vec!["Bash", "Read"]);
}

#[test]
fn test_background_blocks_interactive() {
    let def = make_def(vec![], vec![]);
    let filtered = filter_tools_for_agent(&all_tools(), &def, true);
    // AskUserQuestion is system-blocked for ALL subagents (foreground + background)
    assert!(!filtered.contains(&"AskUserQuestion".to_string()));
}

#[test]
fn test_ask_user_blocked_for_all_subagents() {
    let def = make_def(vec![], vec![]);
    // AskUserQuestion is blocked even for foreground subagents
    let filtered = filter_tools_for_agent(&all_tools(), &def, false);
    assert!(!filtered.contains(&"AskUserQuestion".to_string()));
}

#[test]
fn test_empty_tools_in() {
    let def = make_def(vec![], vec![]);
    let filtered = filter_tools_for_agent(&[], &def, false);
    assert!(filtered.is_empty());
}
