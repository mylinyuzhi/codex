use super::*;

#[test]
fn test_code_simplifier_agent() {
    let agent = code_simplifier_agent();
    assert_eq!(agent.name, "code-simplifier");
    assert_eq!(agent.agent_type, "code-simplifier");
    assert!(
        agent.tools.is_empty(),
        "should have all tools (empty = all)"
    );
    assert!(agent.disallowed_tools.is_empty());
    assert!(agent.identity.is_none(), "should inherit parent model");
    assert!(agent.max_turns.is_none());
    assert!(!agent.fork_context);
    assert_eq!(agent.color.as_deref(), Some("magenta"));
    assert!(agent.critical_reminder.is_none());
}
