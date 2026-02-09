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
    };
    assert!(matches!(
        def.identity,
        Some(ExecutionIdentity::Role(ModelRole::Explore))
    ));
}
