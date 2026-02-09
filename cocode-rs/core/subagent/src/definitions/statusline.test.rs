use super::*;
use cocode_protocol::execution::ExecutionIdentity;

#[test]
fn test_statusline_agent() {
    let agent = statusline_agent();
    assert_eq!(agent.name, "statusline");
    assert_eq!(agent.agent_type, "statusline");
    assert_eq!(agent.tools, vec!["Read", "Edit"]);
    assert!(agent.disallowed_tools.is_empty());
    assert_eq!(agent.max_turns, Some(5));
    assert!(matches!(agent.identity, Some(ExecutionIdentity::Inherit)));
}
