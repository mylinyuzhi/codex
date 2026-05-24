use super::*;
use cocode_protocol::PermissionMode;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelRole;

#[test]
fn test_guide_agent() {
    let agent = guide_agent();
    assert_eq!(agent.name, "guide");
    assert_eq!(agent.agent_type, "guide");
    assert_eq!(
        agent.tools,
        vec![
            ToolName::Glob.as_str(),
            ToolName::Grep.as_str(),
            ToolName::Read.as_str(),
            ToolName::WebFetch.as_str(),
            ToolName::WebSearch.as_str()
        ]
    );
    assert!(agent.disallowed_tools.is_empty());
    assert_eq!(agent.max_turns, Some(15));
    assert!(matches!(
        agent.identity,
        Some(ExecutionIdentity::Role(ModelRole::Fast))
    ));
    assert!(matches!(
        agent.permission_mode,
        Some(PermissionMode::Bypass)
    ));
    assert!(!agent.fork_context);
    assert_eq!(agent.color.as_deref(), Some("green"));
    assert!(agent.critical_reminder.is_some());
}
