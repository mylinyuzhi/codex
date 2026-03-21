use super::*;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelRole;

#[test]
fn test_plan_agent() {
    let agent = plan_agent();
    assert_eq!(agent.name, "plan");
    assert_eq!(agent.agent_type, "plan");
    assert!(
        agent.tools.is_empty(),
        "plan agent uses deny-list, not allow-list"
    );
    assert_eq!(
        agent.disallowed_tools,
        vec![
            ToolName::Edit.as_str(),
            ToolName::Write.as_str(),
            ToolName::NotebookEdit.as_str()
        ]
    );
    assert!(agent.max_turns.is_none());
    assert!(matches!(
        agent.identity,
        Some(ExecutionIdentity::Role(ModelRole::Plan))
    ));
    assert!(agent.permission_mode.is_none());
    assert!(!agent.fork_context);
    assert_eq!(agent.color.as_deref(), Some("blue"));
    assert!(agent.critical_reminder.is_some());
}
