use super::*;
use cocode_protocol::PermissionMode;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelRole;

#[test]
fn test_explore_agent() {
    let agent = explore_agent();
    assert_eq!(agent.name, "explore");
    assert_eq!(agent.agent_type, "explore");
    assert!(
        agent.tools.is_empty(),
        "explore agent uses deny-list, not allow-list"
    );
    assert_eq!(
        agent.disallowed_tools,
        vec![
            ToolName::Edit.as_str(),
            ToolName::Write.as_str(),
            ToolName::NotebookEdit.as_str()
        ]
    );
    assert_eq!(agent.max_turns, Some(20));
    assert!(matches!(
        agent.identity,
        Some(ExecutionIdentity::Role(ModelRole::Explore))
    ));
    assert!(matches!(
        agent.permission_mode,
        Some(PermissionMode::Bypass)
    ));
    assert!(!agent.fork_context);
    assert_eq!(agent.color.as_deref(), Some("cyan"));
    assert!(agent.critical_reminder.is_some());
}
