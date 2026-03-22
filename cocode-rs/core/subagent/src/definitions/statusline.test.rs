use super::*;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelRole;

#[test]
fn test_statusline_agent() {
    let agent = statusline_agent();
    assert_eq!(agent.name, "statusline");
    assert_eq!(agent.agent_type, "statusline");
    assert_eq!(
        agent.tools,
        vec![ToolName::Read.as_str(), ToolName::Edit.as_str()]
    );
    assert!(agent.disallowed_tools.is_empty());
    assert_eq!(agent.max_turns, Some(5));
    assert!(matches!(
        agent.identity,
        Some(ExecutionIdentity::Role(ModelRole::Fast))
    ));
    assert!(!agent.fork_context);
    assert_eq!(agent.color.as_deref(), Some("orange"));
    assert!(agent.critical_reminder.is_none());
}
