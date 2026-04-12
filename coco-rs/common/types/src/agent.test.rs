use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_subagent_type_roundtrip() {
    assert_eq!(SubagentType::Explore.as_str(), "explore");
    assert_eq!(SubagentType::StatusLine.as_str(), "statusline-setup");
    assert_eq!(
        SubagentType::from_str("explore").unwrap(),
        SubagentType::Explore
    );
    assert_eq!(
        SubagentType::from_str("statusline-setup").unwrap(),
        SubagentType::StatusLine
    );
}

#[test]
fn test_subagent_type_kebab_and_snake() {
    // Both kebab-case and snake_case should work
    assert_eq!(
        SubagentType::from_str("claude-code-guide").unwrap(),
        SubagentType::ClaudeCodeGuide
    );
    assert_eq!(
        SubagentType::from_str("claude_code_guide").unwrap(),
        SubagentType::ClaudeCodeGuide
    );
}

#[test]
fn test_agent_type_id_builtin() {
    let id: AgentTypeId = "explore".parse().unwrap();
    assert_eq!(id, AgentTypeId::Builtin(SubagentType::Explore));
    assert_eq!(id.to_string(), "explore");
}

#[test]
fn test_agent_type_id_custom() {
    let id: AgentTypeId = "my-custom-agent".parse().unwrap();
    assert_eq!(id, AgentTypeId::Custom("my-custom-agent".into()));
}

#[test]
fn test_agent_type_id_serde_roundtrip() {
    let id = AgentTypeId::Builtin(SubagentType::Plan);
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "\"plan\"");
    let parsed: AgentTypeId = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, id);
}

// ── AgentIsolation ──

#[test]
fn test_agent_isolation_serde_roundtrip() {
    for (variant, expected_json) in [
        (AgentIsolation::None, "\"none\""),
        (AgentIsolation::Worktree, "\"worktree\""),
        (AgentIsolation::Remote, "\"remote\""),
    ] {
        let json = serde_json::to_string(&variant).unwrap();
        assert_eq!(json, expected_json);
        let parsed: AgentIsolation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, variant);
    }
}

#[test]
fn test_agent_isolation_from_str() {
    assert_eq!(
        AgentIsolation::from_str("none").unwrap(),
        AgentIsolation::None
    );
    assert_eq!(
        AgentIsolation::from_str("worktree").unwrap(),
        AgentIsolation::Worktree
    );
    assert_eq!(
        AgentIsolation::from_str("remote").unwrap(),
        AgentIsolation::Remote
    );
    assert!(AgentIsolation::from_str("invalid").is_err());
}

#[test]
fn test_agent_isolation_default() {
    assert_eq!(AgentIsolation::default(), AgentIsolation::None);
}

// ── MemoryScope ──

#[test]
fn test_memory_scope_serde_roundtrip() {
    for (variant, expected_json) in [
        (MemoryScope::User, "\"user\""),
        (MemoryScope::Project, "\"project\""),
        (MemoryScope::Local, "\"local\""),
    ] {
        let json = serde_json::to_string(&variant).unwrap();
        assert_eq!(json, expected_json);
        let parsed: MemoryScope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, variant);
    }
}

#[test]
fn test_memory_scope_from_str() {
    assert_eq!(MemoryScope::from_str("user").unwrap(), MemoryScope::User);
    assert_eq!(
        MemoryScope::from_str("project").unwrap(),
        MemoryScope::Project
    );
    assert_eq!(MemoryScope::from_str("local").unwrap(), MemoryScope::Local);
    assert!(MemoryScope::from_str("global").is_err());
}

#[test]
fn test_memory_scope_default() {
    assert_eq!(MemoryScope::default(), MemoryScope::Project);
}

// ── ModelInheritance ──

#[test]
fn test_model_inheritance_serde_roundtrip() {
    let inheritance = ModelInheritance {
        model: "opus-4".into(),
        source: ModelSource::Param,
    };
    let json = serde_json::to_string(&inheritance).unwrap();
    let parsed: ModelInheritance = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, inheritance);
}

#[test]
fn test_model_source_variants() {
    for (variant, expected) in [
        (ModelSource::Param, "param"),
        (ModelSource::Definition, "definition"),
        (ModelSource::Parent, "parent"),
    ] {
        assert_eq!(variant.to_string(), expected);
    }
}

// ── AgentDefinition ──

#[test]
fn test_agent_definition_default() {
    let def = AgentDefinition::default();
    assert!(!def.use_exact_tools);
    assert_eq!(def.isolation, AgentIsolation::None);
    assert!(def.mcp_servers.is_empty());
    assert!(def.disallowed_tools.is_empty());
    assert!(def.allowed_tools.is_empty());
    assert!(def.effort.is_none());
    assert!(def.model.is_none());
    assert!(def.memory_scope.is_none());
    assert!(def.initial_prompt.is_none());
    assert!(def.max_turns.is_none());
}

#[test]
fn test_agent_definition_serde_roundtrip() {
    let def = AgentDefinition {
        agent_type: AgentTypeId::Builtin(SubagentType::Explore),
        name: "researcher".into(),
        description: Some("Explores the codebase".into()),
        effort: Some("high".into()),
        use_exact_tools: true,
        model: Some("opus-4".into()),
        isolation: AgentIsolation::Worktree,
        memory_scope: Some(MemoryScope::Project),
        mcp_servers: vec!["github".into(), "jira".into()],
        initial_prompt: Some("Search the codebase for patterns.".into()),
        max_turns: Some(10),
        disallowed_tools: vec!["Bash".into()],
        allowed_tools: vec!["Read".into(), "Grep".into()],
        identity: Some("You are a code researcher.".into()),
        ..Default::default()
    };

    let json = serde_json::to_string(&def).unwrap();
    let parsed: AgentDefinition = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.agent_type, def.agent_type);
    assert_eq!(parsed.name, "researcher");
    assert_eq!(parsed.effort.as_deref(), Some("high"));
    assert!(parsed.use_exact_tools);
    assert_eq!(parsed.model.as_deref(), Some("opus-4"));
    assert_eq!(parsed.isolation, AgentIsolation::Worktree);
    assert_eq!(parsed.memory_scope, Some(MemoryScope::Project));
    assert_eq!(parsed.mcp_servers, vec!["github", "jira"]);
    assert_eq!(parsed.max_turns, Some(10));
    assert_eq!(parsed.disallowed_tools, vec!["Bash"]);
    assert_eq!(parsed.allowed_tools, vec!["Read", "Grep"]);
}

#[test]
fn test_agent_definition_serde_skip_empty_defaults() {
    let def = AgentDefinition::default();
    let json = serde_json::to_string(&def).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = value.as_object().unwrap();

    // Optional/empty fields should be omitted
    assert!(!obj.contains_key("description"));
    assert!(!obj.contains_key("effort"));
    assert!(!obj.contains_key("model"));
    assert!(!obj.contains_key("memory_scope"));
    assert!(!obj.contains_key("mcp_servers"));
    assert!(!obj.contains_key("initial_prompt"));
    assert!(!obj.contains_key("max_turns"));
    assert!(!obj.contains_key("disallowed_tools"));
    assert!(!obj.contains_key("allowed_tools"));
    assert!(!obj.contains_key("identity"));
}
