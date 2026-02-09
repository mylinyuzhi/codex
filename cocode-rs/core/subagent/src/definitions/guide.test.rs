use super::*;
use cocode_protocol::execution::ExecutionIdentity;

#[test]
fn test_guide_agent() {
    let agent = guide_agent();
    assert_eq!(agent.name, "guide");
    assert_eq!(agent.agent_type, "guide");
    assert_eq!(agent.tools, vec!["Glob", "Grep", "Read"]);
    assert!(agent.disallowed_tools.is_empty());
    assert_eq!(agent.max_turns, Some(15));
    assert!(matches!(agent.identity, Some(ExecutionIdentity::Inherit)));
}
