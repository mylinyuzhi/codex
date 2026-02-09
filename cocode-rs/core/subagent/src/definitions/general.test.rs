use super::*;
use cocode_protocol::execution::ExecutionIdentity;

#[test]
fn test_general_agent() {
    let agent = general_agent();
    assert_eq!(agent.name, "general");
    assert_eq!(agent.agent_type, "general");
    assert!(agent.tools.is_empty(), "general agent has all tools");
    assert!(agent.disallowed_tools.is_empty());
    assert!(agent.max_turns.is_none());
    assert!(matches!(agent.identity, Some(ExecutionIdentity::Inherit)));
}
