use super::*;
use cocode_protocol::ToolName;
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
    // New fields default correctly
    assert!(def.skills.is_empty());
    assert!(!def.background);
    assert!(def.memory.is_none());
    assert!(def.hooks.is_none());
    assert!(def.mcp_servers.is_none());
    assert!(def.isolation.is_none());
    assert!(!def.use_custom_prompt);
}

#[test]
fn test_agent_definition_full() {
    let def = AgentDefinition {
        name: "bash".to_string(),
        description: "Bash executor".to_string(),
        agent_type: "bash".to_string(),
        tools: vec![ToolName::Bash.as_str().to_string()],
        disallowed_tools: vec![ToolName::Edit.as_str().to_string()],
        identity: Some(ExecutionIdentity::Role(ModelRole::Main)),
        max_turns: Some(10),
        permission_mode: None,
        fork_context: true,
        color: Some("cyan".to_string()),
        critical_reminder: Some("Do not modify files.".to_string()),
        source: AgentSource::BuiltIn,
        skills: vec!["api-conventions".to_string()],
        background: true,
        memory: Some(MemoryScope::User),
        hooks: None,
        mcp_servers: None,
        isolation: Some(IsolationMode::Worktree),
        use_custom_prompt: false,
    };
    let json = serde_json::to_string(&def).expect("serialize");
    let back: AgentDefinition = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.name, "bash");
    assert_eq!(back.tools, vec![ToolName::Bash.as_str()]);
    assert_eq!(back.disallowed_tools, vec![ToolName::Edit.as_str()]);
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
    assert_eq!(back.skills, vec!["api-conventions"]);
    assert_eq!(back.memory, Some(MemoryScope::User));
    assert!(back.background);
    assert_eq!(back.isolation, Some(IsolationMode::Worktree));
}

#[test]
fn test_agent_definition_with_identity() {
    let def = AgentDefinition {
        agent_type: "explore".into(),
        name: "explore".into(),
        description: "Explorer".into(),
        identity: Some(ExecutionIdentity::Role(ModelRole::Explore)),
        ..Default::default()
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

#[test]
fn test_agent_source_priority() {
    assert!(AgentSource::CliFlag.priority() > AgentSource::ProjectSettings.priority());
    assert!(AgentSource::ProjectSettings.priority() > AgentSource::UserSettings.priority());
    assert!(AgentSource::UserSettings.priority() > AgentSource::Plugin.priority());
    assert!(AgentSource::Plugin.priority() > AgentSource::BuiltIn.priority());
}

#[test]
fn test_agent_source_cli_flag_serde() {
    let json = serde_json::to_string(&AgentSource::CliFlag).expect("serialize");
    let back: AgentSource = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, AgentSource::CliFlag);
}

#[test]
fn test_memory_scope_serde() {
    let json = serde_json::to_string(&MemoryScope::User).expect("serialize");
    assert_eq!(json, r#""user""#);
    let back: MemoryScope = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, MemoryScope::User);

    let json = serde_json::to_string(&MemoryScope::Project).expect("serialize");
    assert_eq!(json, r#""project""#);

    let json = serde_json::to_string(&MemoryScope::Local).expect("serialize");
    assert_eq!(json, r#""local""#);
}

#[test]
fn test_isolation_mode_serde() {
    let json = serde_json::to_string(&IsolationMode::Worktree).expect("serialize");
    assert_eq!(json, r#""worktree""#);
    let back: IsolationMode = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, IsolationMode::Worktree);
}

#[test]
fn test_new_fields_deserialize_defaults() {
    let json = r#"{"name":"t","description":"d","agent_type":"t"}"#;
    let def: AgentDefinition = serde_json::from_str(json).expect("deserialize");
    assert!(def.skills.is_empty());
    assert!(!def.background);
    assert!(def.memory.is_none());
    assert!(def.hooks.is_none());
    assert!(def.mcp_servers.is_none());
    assert!(def.isolation.is_none());
}

#[test]
fn test_default_impl() {
    let def = AgentDefinition::default();
    assert!(def.name.is_empty());
    assert!(def.description.is_empty());
    assert!(def.agent_type.is_empty());
    assert!(def.tools.is_empty());
    assert!(def.disallowed_tools.is_empty());
    assert!(def.identity.is_none());
    assert!(def.max_turns.is_none());
    assert!(def.permission_mode.is_none());
    assert!(!def.fork_context);
    assert!(def.color.is_none());
    assert!(def.critical_reminder.is_none());
    assert_eq!(def.source, AgentSource::BuiltIn);
    assert!(def.skills.is_empty());
    assert!(!def.background);
    assert!(def.memory.is_none());
    assert!(def.hooks.is_none());
    assert!(def.mcp_servers.is_none());
    assert!(def.isolation.is_none());
    assert!(!def.use_custom_prompt);
}

#[test]
fn test_default_with_struct_update() {
    let def = AgentDefinition {
        agent_type: "bash".into(),
        name: "bash".into(),
        description: "Bash executor".into(),
        ..Default::default()
    };
    assert_eq!(def.agent_type, "bash");
    assert!(def.tools.is_empty());
    assert!(!def.fork_context);
}
