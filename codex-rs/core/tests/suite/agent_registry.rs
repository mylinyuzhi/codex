use codex_core::agent_registry::AgentRegistry;
use codex_protocol::agent_definition::{AgentDefinition, AgentLoadStatus};
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn test_builtin_agents_loaded() {
    let registry = AgentRegistry::load();

    assert!(registry.is_available("review"));
    assert!(registry.is_available("compact"));

    let review = registry.get("review").unwrap();
    match review {
        AgentLoadStatus::Available(def) => {
            assert_eq!(def.name, "review");
            assert!(!def.system_prompt.is_empty());
            assert!(def.description.contains("review"));
        }
        AgentLoadStatus::Invalid { .. } => {
            panic!("Expected review agent to be available")
        }
    }
}

#[test]
fn test_load_custom_agent_from_file() {
    let temp_dir = TempDir::new().unwrap();
    let agent_file = temp_dir.path().join("test-agent.toml");

    std::fs::write(
        &agent_file,
        r#"
name = "test-agent"
description = "Test agent for integration testing"
system_prompt = "You are a test agent"
model = "claude-sonnet-4"
tools = ["read_file", "grep"]
max_turns = 10
thinking_budget = 5000
"#,
    )
    .unwrap();

    let mut agents = std::collections::HashMap::new();
    AgentRegistry::load_from_directory(&mut agents, temp_dir.path(), "test");

    assert_eq!(agents.len(), 1);
    let status = agents.get("test-agent").unwrap();

    match status {
        AgentLoadStatus::Available(def) => {
            assert_eq!(def.name, "test-agent");
            assert_eq!(def.model.as_ref().unwrap(), "claude-sonnet-4");
            assert_eq!(def.tools.as_ref().unwrap().len(), 2);
            assert_eq!(def.max_turns, Some(10));
            assert_eq!(def.thinking_budget, Some(5000));
            assert_eq!(def.description, "Test agent for integration testing");
        }
        AgentLoadStatus::Invalid { error, .. } => {
            panic!("Expected valid agent, got Invalid: {}", error)
        }
    }
}

#[test]
fn test_invalid_agent_marked_as_invalid() {
    let temp_dir = TempDir::new().unwrap();
    let agent_file = temp_dir.path().join("bad-agent.toml");

    // Missing required field: system_prompt
    std::fs::write(
        &agent_file,
        r#"
name = "bad-agent"
description = "Bad agent with missing system_prompt"
"#,
    )
    .unwrap();

    let mut agents = std::collections::HashMap::new();
    AgentRegistry::load_from_directory(&mut agents, temp_dir.path(), "test");

    assert_eq!(agents.len(), 1);
    match agents.get("bad-agent").unwrap() {
        AgentLoadStatus::Invalid { error, .. } => {
            assert!(error.contains("system_prompt") || error.contains("missing field"));
        }
        AgentLoadStatus::Available(_) => {
            panic!("Expected Invalid status for bad agent")
        }
    }
}

#[test]
fn test_empty_name_rejected() {
    let temp_dir = TempDir::new().unwrap();
    let agent_file = temp_dir.path().join("empty-name.toml");

    std::fs::write(
        &agent_file,
        r#"
name = ""
description = "Agent with empty name"
system_prompt = "Test prompt"
"#,
    )
    .unwrap();

    let mut agents = std::collections::HashMap::new();
    AgentRegistry::load_from_directory(&mut agents, temp_dir.path(), "test");

    assert_eq!(agents.len(), 1);
    match agents.get("empty-name").unwrap() {
        AgentLoadStatus::Invalid { error, .. } => {
            assert!(error.contains("name cannot be empty"));
        }
        AgentLoadStatus::Available(_) => {
            panic!("Expected Invalid status for empty name")
        }
    }
}

#[test]
fn test_priority_override() {
    // Test that later loads override earlier ones
    let mut agents = std::collections::HashMap::new();

    // Simulate built-in
    agents.insert(
        "review".to_string(),
        AgentLoadStatus::Available(Arc::new(AgentDefinition {
            name: "review".to_string(),
            description: "Built-in review agent".to_string(),
            system_prompt: "Built-in prompt".to_string(),
            model: None,
            tools: None,
            max_turns: None,
            thinking_budget: None,
        })),
    );

    let original_count = agents.len();

    // Override with custom
    let temp_dir = TempDir::new().unwrap();
    let agent_file = temp_dir.path().join("review.toml");
    std::fs::write(
        &agent_file,
        r#"
name = "review"
description = "Custom review agent"
system_prompt = "Custom prompt for review"
model = "claude-opus-4"
"#,
    )
    .unwrap();

    AgentRegistry::load_from_directory(&mut agents, temp_dir.path(), "custom");

    // Should still have same number of agents (override, not add)
    assert_eq!(agents.len(), original_count);

    let custom_review = match agents.get("review").unwrap() {
        AgentLoadStatus::Available(def) => Arc::clone(def),
        _ => panic!("Expected available"),
    };

    // Verify override
    assert_eq!(custom_review.system_prompt, "Custom prompt for review");
    assert_eq!(custom_review.model.as_ref().unwrap(), "claude-opus-4");
}

#[test]
fn test_list_available_excludes_invalid() {
    // Create temp directory with valid and invalid agents
    let temp_dir = TempDir::new().unwrap();

    // Valid agent 1
    std::fs::write(
        temp_dir.path().join("valid1.toml"),
        r#"
name = "valid1"
description = "Valid agent 1"
system_prompt = "Prompt 1"
"#,
    )
    .unwrap();

    // Invalid agent (missing system_prompt)
    std::fs::write(
        temp_dir.path().join("invalid1.toml"),
        r#"
name = "invalid1"
description = "Invalid agent"
"#,
    )
    .unwrap();

    // Valid agent 2
    std::fs::write(
        temp_dir.path().join("valid2.toml"),
        r#"
name = "valid2"
description = "Valid agent 2"
system_prompt = "Prompt 2"
"#,
    )
    .unwrap();

    let mut agents = std::collections::HashMap::new();
    AgentRegistry::load_from_directory(&mut agents, temp_dir.path(), "test");

    // Should have 3 agents total (2 valid, 1 invalid)
    assert_eq!(agents.len(), 3);

    // Filter to get only available ones
    let available: Vec<Arc<AgentDefinition>> = agents
        .values()
        .filter_map(|status| match status {
            AgentLoadStatus::Available(def) => Some(Arc::clone(def)),
            AgentLoadStatus::Invalid { .. } => None,
        })
        .collect();

    assert_eq!(available.len(), 2);
    assert!(available.iter().any(|def| def.name == "valid1"));
    assert!(available.iter().any(|def| def.name == "valid2"));
    assert!(!available.iter().any(|def| def.name == "invalid1"));
}

#[test]
fn test_tool_filtering() {
    let agent = AgentDefinition {
        name: "restricted".to_string(),
        description: "Test".to_string(),
        system_prompt: "Test".to_string(),
        model: None,
        tools: Some(vec!["read_file".to_string(), "grep".to_string()]),
        max_turns: None,
        thinking_budget: None,
    };

    assert!(agent.is_tool_allowed("read_file"));
    assert!(agent.is_tool_allowed("grep"));
    assert!(!agent.is_tool_allowed("shell"));
    assert!(!agent.is_tool_allowed("write_file"));
}

#[test]
fn test_no_tool_restrictions() {
    let agent = AgentDefinition {
        name: "unrestricted".to_string(),
        description: "Test".to_string(),
        system_prompt: "Test".to_string(),
        model: None,
        tools: None,
        max_turns: None,
        thinking_budget: None,
    };

    assert!(agent.is_tool_allowed("read_file"));
    assert!(agent.is_tool_allowed("shell"));
    assert!(agent.is_tool_allowed("anything"));
}
