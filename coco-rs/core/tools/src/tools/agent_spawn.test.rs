use super::*;

#[test]
fn test_builtin_agent_check() {
    assert!(is_builtin_agent("general-purpose"));
    assert!(is_builtin_agent("Explore"));
    assert!(is_builtin_agent("Plan"));
    assert!(!is_builtin_agent("my-custom-agent"));
}

#[test]
fn test_filter_agents_by_mcp() {
    let agents = vec![
        AgentDefinition {
            name: "no-mcp".into(),
            ..general_purpose_agent()
        },
        AgentDefinition {
            name: "needs-slack".into(),
            required_mcp_servers: vec!["slack".into()],
            ..general_purpose_agent()
        },
    ];

    let available = vec!["github".to_string()];
    let filtered = filter_agents_by_mcp(&agents, &available);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "no-mcp");

    let available_with_slack = vec!["github".to_string(), "slack".to_string()];
    let filtered = filter_agents_by_mcp(&agents, &available_with_slack);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn test_parse_agent_definition_no_frontmatter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test-agent.md");
    std::fs::write(&path, "You are a helpful agent.").unwrap();

    let agent = parse_agent_definition(&path).unwrap();
    assert_eq!(agent.name, "test-agent");
    assert_eq!(
        agent.initial_prompt.as_deref(),
        Some("You are a helpful agent.")
    );
}

#[test]
fn test_parse_agent_definition_with_frontmatter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("custom.md");
    std::fs::write(
        &path,
        "---\nname: my-agent\ndescription: A custom agent\nmodel: sonnet\n---\nYou are custom.",
    )
    .unwrap();

    let agent = parse_agent_definition(&path).unwrap();
    assert_eq!(agent.name, "my-agent");
    assert_eq!(agent.description.as_deref(), Some("A custom agent"));
    assert_eq!(agent.model, Some("sonnet".to_string()));
    assert_eq!(agent.initial_prompt.as_deref(), Some("You are custom."));
}

#[test]
fn test_general_purpose_agent_defaults() {
    let agent = general_purpose_agent();
    assert_eq!(agent.name, "general-purpose");
    assert_eq!(agent.max_turns, Some(30));
    assert!(agent.description.is_some());
}

#[test]
fn test_is_builtin_definition() {
    let builtin = AgentDefinition {
        agent_type: AgentTypeId::Builtin(SubagentType::Explore),
        name: "Explore".into(),
        ..Default::default()
    };
    assert!(is_builtin_definition(&builtin));

    let custom = AgentDefinition {
        agent_type: AgentTypeId::Custom("my-agent".into()),
        name: "my-agent".into(),
        ..Default::default()
    };
    assert!(!is_builtin_definition(&custom));
}
