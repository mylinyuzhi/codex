use super::*;
use cocode_protocol::model::ModelRole;

#[test]
fn test_agent_definition_defaults() {
    let json = r#"{"name":"test","description":"A test agent","agent_type":"test"}"#;
    let def: AgentDefinition = serde_json::from_str(json).expect("deserialize");
    assert_eq!(def.name, "test");
    assert!(def.tools.is_empty());
    assert!(def.disallowed_tools.is_empty());
    assert!(def.identity.is_none());
    assert!(def.max_turns.is_none());
    assert!(!def.fork_context);
    assert!(def.color.is_none());
    assert!(def.critical_reminder.is_none());
    assert_eq!(def.source, AgentSource::BuiltIn);
}

#[test]
fn test_agent_definition_full() {
    let def = AgentDefinition {
        name: "bash".to_string(),
        description: "Bash executor".to_string(),
        agent_type: "bash".to_string(),
        tools: vec!["Bash".to_string()],
        disallowed_tools: vec!["Edit".to_string()],
        identity: Some(ExecutionIdentity::Role(ModelRole::Main)),
        max_turns: Some(10),
        permission_mode: None,
        fork_context: true,
        color: Some("cyan".to_string()),
        critical_reminder: Some("Do not modify files.".to_string()),
        source: AgentSource::BuiltIn,
    };
    let json = serde_json::to_string(&def).expect("serialize");
    let back: AgentDefinition = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.name, "bash");
    assert_eq!(back.tools, vec!["Bash"]);
    assert_eq!(back.disallowed_tools, vec!["Edit"]);
    assert!(matches!(
        back.identity,
        Some(ExecutionIdentity::Role(ModelRole::Main))
    ));
    assert_eq!(back.max_turns, Some(10));
    assert!(back.fork_context);
    assert_eq!(back.color.as_deref(), Some("cyan"));
    assert_eq!(
        back.critical_reminder.as_deref(),
        Some("Do not modify files.")
    );
    assert_eq!(back.source, AgentSource::BuiltIn);
}

#[test]
fn test_agent_definition_with_identity() {
    let def = AgentDefinition {
        name: "explore".to_string(),
        description: "Explorer".to_string(),
        agent_type: "explore".to_string(),
        tools: vec![],
        disallowed_tools: vec![],
        identity: Some(ExecutionIdentity::Role(ModelRole::Explore)),
        max_turns: None,
        permission_mode: None,
        fork_context: false,
        color: None,
        critical_reminder: None,
        source: AgentSource::BuiltIn,
    };
    assert!(matches!(
        def.identity,
        Some(ExecutionIdentity::Role(ModelRole::Explore))
    ));
}

#[test]
fn test_agent_source_serde() {
    let json = serde_json::to_string(&AgentSource::Plugin).expect("serialize");
    let back: AgentSource = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, AgentSource::Plugin);
}
