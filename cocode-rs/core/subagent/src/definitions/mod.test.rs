use super::*;
use cocode_config::BuiltinAgentOverride;
use cocode_config::BuiltinAgentsConfig;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelRole;

#[test]
fn test_builtin_agents_count() {
    let agents = builtin_agents();
    assert_eq!(agents.len(), 6);
}

#[test]
fn test_builtin_agents_unique_names() {
    let agents = builtin_agents();
    let mut names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 6, "All agent names should be unique");
}

#[test]
fn test_builtin_agent_types() {
    let agents = builtin_agents();
    let types: Vec<&str> = agents.iter().map(|a| a.agent_type.as_str()).collect();
    assert!(types.contains(&"bash"));
    assert!(types.contains(&"general"));
    assert!(types.contains(&"explore"));
    assert!(types.contains(&"plan"));
    assert!(types.contains(&"guide"));
    assert!(types.contains(&"statusline"));
}

#[test]
fn test_builtin_agents_with_empty_config() {
    let config = BuiltinAgentsConfig::new();
    let agents = builtin_agents_with_config(&config);
    assert_eq!(agents.len(), 6);

    // Should be unchanged from defaults
    let explore = agents.iter().find(|a| a.agent_type == "explore").unwrap();
    assert_eq!(explore.max_turns, Some(20));
}

#[test]
fn test_builtin_agents_with_max_turns_override() {
    let mut config = BuiltinAgentsConfig::new();
    config.insert(
        "explore".to_string(),
        BuiltinAgentOverride {
            max_turns: Some(50),
            identity: None,
            tools: None,
            disallowed_tools: None,
        },
    );

    let agents = builtin_agents_with_config(&config);
    let explore = agents.iter().find(|a| a.agent_type == "explore").unwrap();
    assert_eq!(explore.max_turns, Some(50));
}

#[test]
fn test_builtin_agents_with_identity_override() {
    let mut config = BuiltinAgentsConfig::new();
    config.insert(
        "explore".to_string(),
        BuiltinAgentOverride {
            max_turns: None,
            identity: Some("fast".to_string()),
            tools: None,
            disallowed_tools: None,
        },
    );

    let agents = builtin_agents_with_config(&config);
    let explore = agents.iter().find(|a| a.agent_type == "explore").unwrap();
    assert!(matches!(
        explore.identity,
        Some(ExecutionIdentity::Role(ModelRole::Fast))
    ));
}

#[test]
fn test_builtin_agents_with_tools_override() {
    let mut config = BuiltinAgentsConfig::new();
    config.insert(
        "explore".to_string(),
        BuiltinAgentOverride {
            max_turns: None,
            identity: None,
            tools: Some(vec!["Read".to_string(), "Bash".to_string()]),
            disallowed_tools: None,
        },
    );

    let agents = builtin_agents_with_config(&config);
    let explore = agents.iter().find(|a| a.agent_type == "explore").unwrap();
    assert_eq!(explore.tools, vec!["Read", "Bash"]);
}

#[test]
fn test_builtin_agents_unknown_agent_ignored() {
    let mut config = BuiltinAgentsConfig::new();
    config.insert(
        "unknown_agent".to_string(),
        BuiltinAgentOverride {
            max_turns: Some(999),
            identity: None,
            tools: None,
            disallowed_tools: None,
        },
    );

    let agents = builtin_agents_with_config(&config);
    // Should still have 6 agents, unknown config is ignored
    assert_eq!(agents.len(), 6);
}

#[test]
fn test_parse_identity_roles() {
    assert!(matches!(
        parse_identity("main"),
        ExecutionIdentity::Role(ModelRole::Main)
    ));
    assert!(matches!(
        parse_identity("fast"),
        ExecutionIdentity::Role(ModelRole::Fast)
    ));
    assert!(matches!(
        parse_identity("explore"),
        ExecutionIdentity::Role(ModelRole::Explore)
    ));
    assert!(matches!(
        parse_identity("plan"),
        ExecutionIdentity::Role(ModelRole::Plan)
    ));
    assert!(matches!(
        parse_identity("vision"),
        ExecutionIdentity::Role(ModelRole::Vision)
    ));
    assert!(matches!(
        parse_identity("review"),
        ExecutionIdentity::Role(ModelRole::Review)
    ));
    assert!(matches!(
        parse_identity("compact"),
        ExecutionIdentity::Role(ModelRole::Compact)
    ));
}

#[test]
fn test_parse_identity_inherit() {
    assert!(matches!(
        parse_identity("inherit"),
        ExecutionIdentity::Inherit
    ));
    assert!(matches!(
        parse_identity("unknown"),
        ExecutionIdentity::Inherit
    ));
    assert!(matches!(parse_identity(""), ExecutionIdentity::Inherit));
}

#[test]
fn test_parse_identity_case_insensitive() {
    assert!(matches!(
        parse_identity("MAIN"),
        ExecutionIdentity::Role(ModelRole::Main)
    ));
    assert!(matches!(
        parse_identity("Fast"),
        ExecutionIdentity::Role(ModelRole::Fast)
    ));
    assert!(matches!(
        parse_identity("EXPLORE"),
        ExecutionIdentity::Role(ModelRole::Explore)
    ));
}
