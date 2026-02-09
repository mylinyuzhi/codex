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
        "plan agent can use all non-denied tools"
    );
    assert_eq!(agent.disallowed_tools, vec!["Task", "Edit", "Write"]);
    assert!(agent.max_turns.is_none());
    assert!(matches!(
        agent.identity,
        Some(ExecutionIdentity::Role(ModelRole::Plan))
    ));
}
