use cocode_protocol::ToolName;

use super::*;

#[test]
fn test_is_delegate_tool() {
    assert!(is_delegate_tool(ToolName::TeamCreate.as_str()));
    assert!(is_delegate_tool(ToolName::TeamDelete.as_str()));
    assert!(is_delegate_tool(ToolName::SendMessage.as_str()));
    assert!(is_delegate_tool(ToolName::TaskCreate.as_str()));
    assert!(is_delegate_tool(ToolName::TaskGet.as_str()));
    assert!(is_delegate_tool(ToolName::TaskUpdate.as_str()));
    assert!(is_delegate_tool(ToolName::TaskList.as_str()));
    assert!(is_delegate_tool(ToolName::Task.as_str()));

    assert!(!is_delegate_tool(ToolName::Read.as_str()));
    assert!(!is_delegate_tool(ToolName::Write.as_str()));
    assert!(!is_delegate_tool(ToolName::Edit.as_str()));
    assert!(!is_delegate_tool(ToolName::Bash.as_str()));
    assert!(!is_delegate_tool(ToolName::Grep.as_str()));
    assert!(!is_delegate_tool(ToolName::Glob.as_str()));
}

#[test]
fn test_filter_for_delegate_mode() {
    let all_tools: Vec<String> = vec![
        ToolName::Read.as_str(),
        ToolName::Write.as_str(),
        ToolName::Edit.as_str(),
        ToolName::Bash.as_str(),
        ToolName::Grep.as_str(),
        ToolName::Glob.as_str(),
        ToolName::TeamCreate.as_str(),
        ToolName::TeamDelete.as_str(),
        ToolName::SendMessage.as_str(),
        ToolName::TaskCreate.as_str(),
        ToolName::TaskGet.as_str(),
        ToolName::TaskUpdate.as_str(),
        ToolName::TaskList.as_str(),
        ToolName::Task.as_str(),
        ToolName::WebFetch.as_str(),
        ToolName::WebSearch.as_str(),
    ]
    .into_iter()
    .map(String::from)
    .collect();

    let filtered = filter_for_delegate_mode(&all_tools);
    assert_eq!(filtered.len(), DELEGATE_MODE_TOOLS.len());

    for tool in &filtered {
        assert!(is_delegate_tool(tool), "{tool} should be a delegate tool");
    }
}

#[test]
fn test_delegate_mode_state_serde() {
    let state = DelegateModeState {
        active: true,
        team_name: "alpha".to_string(),
        agent_id: "lead-01".to_string(),
    };
    let json = serde_json::to_string(&state).unwrap();
    let restored: DelegateModeState = serde_json::from_str(&json).unwrap();
    assert!(restored.active);
    assert_eq!(restored.team_name, "alpha");
    assert_eq!(restored.agent_id, "lead-01");
}
