use super::*;
use std::fs;

#[test]
fn test_load_agents_from_empty_dir() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let results = load_agents_from_dir(tmp.path(), "test-plugin");
    assert!(results.is_empty());
}

#[test]
fn test_load_agents_from_nonexistent_dir() {
    let results = load_agents_from_dir(Path::new("/nonexistent"), "test-plugin");
    assert!(results.is_empty());
}

#[test]
fn test_load_agent_from_dir() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let agent_dir = tmp.path().join("my-agent");
    fs::create_dir_all(&agent_dir).expect("mkdir");

    fs::write(
        agent_dir.join("AGENT.toml"),
        r#"
name = "my-agent"
description = "A test agent"
agent_type = "my-agent"
tools = ["Read", "Glob"]
disallowed_tools = ["Write"]
max_turns = 10
"#,
    )
    .expect("write");

    let results = load_agents_from_dir(tmp.path(), "test-plugin");
    assert_eq!(results.len(), 1);

    if let PluginContribution::Agent {
        definition,
        plugin_name,
    } = &results[0]
    {
        assert_eq!(definition.name, "my-agent");
        assert_eq!(definition.description, "A test agent");
        assert_eq!(definition.agent_type, "my-agent");
        assert_eq!(definition.tools, vec!["Read", "Glob"]);
        assert_eq!(definition.disallowed_tools, vec!["Write"]);
        assert_eq!(definition.max_turns, Some(10));
        assert_eq!(plugin_name, "test-plugin");
    } else {
        panic!("Expected Agent contribution");
    }
}

#[test]
fn test_load_agent_minimal() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let agent_dir = tmp.path().join("minimal");
    fs::create_dir_all(&agent_dir).expect("mkdir");

    fs::write(
        agent_dir.join("AGENT.toml"),
        r#"
name = "minimal"
description = "Minimal agent"
agent_type = "minimal"
"#,
    )
    .expect("write");

    let results = load_agents_from_dir(tmp.path(), "test-plugin");
    assert_eq!(results.len(), 1);

    if let PluginContribution::Agent { definition, .. } = &results[0] {
        assert_eq!(definition.name, "minimal");
        assert!(definition.tools.is_empty());
        assert!(definition.disallowed_tools.is_empty());
        assert!(definition.max_turns.is_none());
    } else {
        panic!("Expected Agent contribution");
    }
}

#[test]
fn test_load_agent_invalid_toml() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let agent_dir = tmp.path().join("invalid");
    fs::create_dir_all(&agent_dir).expect("mkdir");

    fs::write(agent_dir.join("AGENT.toml"), "invalid { toml").expect("write");

    let results = load_agents_from_dir(tmp.path(), "test-plugin");
    assert!(results.is_empty()); // Invalid TOML should be skipped
}

#[test]
fn test_load_multiple_agents() {
    let tmp = tempfile::tempdir().expect("create temp dir");

    // Agent 1
    let agent1 = tmp.path().join("agent1");
    fs::create_dir_all(&agent1).expect("mkdir");
    fs::write(
        agent1.join("AGENT.toml"),
        r#"
name = "agent1"
description = "First agent"
agent_type = "agent1"
"#,
    )
    .expect("write");

    // Agent 2
    let agent2 = tmp.path().join("agent2");
    fs::create_dir_all(&agent2).expect("mkdir");
    fs::write(
        agent2.join("AGENT.toml"),
        r#"
name = "agent2"
description = "Second agent"
agent_type = "agent2"
"#,
    )
    .expect("write");

    let results = load_agents_from_dir(tmp.path(), "test-plugin");
    assert_eq!(results.len(), 2);

    let names: Vec<&str> = results
        .iter()
        .filter_map(|c| {
            if let PluginContribution::Agent { definition, .. } = c {
                Some(definition.name.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(names.contains(&"agent1"));
    assert!(names.contains(&"agent2"));
}
