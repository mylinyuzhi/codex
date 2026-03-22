use super::*;
use cocode_protocol::execution::ExecutionIdentity;

#[test]
fn test_bash_agent() {
    let agent = bash_agent();
    assert_eq!(agent.name, "bash");
    assert_eq!(agent.agent_type, "bash");
    assert_eq!(agent.tools, vec![ToolName::Bash.as_str()]);
    assert!(agent.disallowed_tools.is_empty());
    assert_eq!(agent.max_turns, Some(10));
    assert!(matches!(agent.identity, Some(ExecutionIdentity::Inherit)));
    assert!(!agent.fork_context);
    assert!(agent.color.is_none());
    assert!(agent.critical_reminder.is_none());
}
