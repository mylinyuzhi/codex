use super::*;
use cocode_config::BuiltinAgentOverride;
use cocode_config::BuiltinAgentsConfig;
use cocode_protocol::ToolName;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelRole;

#[test]
fn test_builtin_agents_count() {
    let agents = builtin_agents();
    assert_eq!(agents.len(), 7);
}

#[test]
fn test_builtin_agents_unique_names() {
    let agents = builtin_agents();
    let mut names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 7, "All agent names should be unique");
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
    assert!(types.contains(&"code-simplifier"));
}

#[test]
fn test_builtin_agents_with_empty_config() {
    let config = BuiltinAgentsConfig::new();
    let agents = builtin_agents_with_config(&config);
    assert_eq!(agents.len(), 7);

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
            ..Default::default()
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
            identity: Some("fast".to_string()),
            ..Default::default()
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
            tools: Some(vec![
                ToolName::Read.as_str().to_string(),
                ToolName::Bash.as_str().to_string(),
            ]),
            ..Default::default()
        },
    );

    let agents = builtin_agents_with_config(&config);
    let explore = agents.iter().find(|a| a.agent_type == "explore").unwrap();
    assert_eq!(
        explore.tools,
        vec![ToolName::Read.as_str(), ToolName::Bash.as_str()]
    );
}

#[test]
fn test_builtin_agents_unknown_agent_ignored() {
    let mut config = BuiltinAgentsConfig::new();
    config.insert(
        "unknown_agent".to_string(),
        BuiltinAgentOverride {
            max_turns: Some(999),
            ..Default::default()
        },
    );

    let agents = builtin_agents_with_config(&config);
    // Should still have 6 agents, unknown config is ignored
    assert_eq!(agents.len(), 7);
}

// parse_identity tests moved to cocode_protocol::execution::identity.test.rs
// (ExecutionIdentity::parse_loose)
