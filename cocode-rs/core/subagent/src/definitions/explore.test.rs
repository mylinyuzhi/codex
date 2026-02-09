use super::*;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelRole;

#[test]
fn test_explore_agent() {
    let agent = explore_agent();
    assert_eq!(agent.name, "explore");
    assert_eq!(agent.agent_type, "explore");
    assert_eq!(agent.tools, vec!["Read", "Glob", "Grep"]);
    assert!(agent.disallowed_tools.is_empty());
    assert_eq!(agent.max_turns, Some(20));
    assert!(matches!(
        agent.identity,
        Some(ExecutionIdentity::Role(ModelRole::Explore))
    ));
}
