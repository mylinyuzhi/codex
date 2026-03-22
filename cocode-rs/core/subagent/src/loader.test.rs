use super::*;
use cocode_protocol::ToolName;
use std::io::Write;
use tempfile::TempDir;

fn write_agent_file(dir: &Path, filename: &str, content: &str) {
    let path = dir.join(filename);
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(content.as_bytes()).unwrap();
}

#[test]
fn test_parse_frontmatter_basic() {
    let content = "---\nname: test\ndescription: A test agent\n---\nBody text here.\n";
    let (yaml, body) = parse_frontmatter(content).unwrap();
    assert!(yaml.contains("name: test"));
    assert!(yaml.contains("description: A test agent"));
    assert_eq!(body.trim(), "Body text here.");
}

#[test]
fn test_parse_frontmatter_no_body() {
    let content = "---\nname: test\n---\n";
    let (yaml, body) = parse_frontmatter(content).unwrap();
    assert!(yaml.contains("name: test"));
    assert!(body.trim().is_empty());
}

#[test]
fn test_parse_frontmatter_missing_opening() {
    let content = "name: test\n---\nBody\n";
    assert!(parse_frontmatter(content).is_err());
}

#[test]
fn test_parse_frontmatter_missing_closing() {
    let content = "---\nname: test\n";
    assert!(parse_frontmatter(content).is_err());
}

#[test]
fn test_load_agent_from_file_full() {
    let dir = TempDir::new().unwrap();
    let content = "\
---
name: my-agent
description: My custom agent
model: fast
tools:
  - Read
  - Glob
  - Grep
disallowedTools:
  - Edit
maxTurns: 15
permissionMode: bypass
forkContext: false
color: cyan
---
CRITICAL: This is a custom read-only agent.
";
    write_agent_file(dir.path(), "my-agent.md", content);

    let path = dir.path().join("my-agent.md");
    let def = load_agent_from_file(&path, AgentSource::UserSettings).unwrap();

    assert_eq!(def.name, "my-agent");
    assert_eq!(def.agent_type, "my-agent");
    assert_eq!(def.description, "My custom agent");
    assert_eq!(
        def.tools,
        vec![
            ToolName::Read.as_str(),
            ToolName::Glob.as_str(),
            ToolName::Grep.as_str()
        ]
    );
    assert_eq!(def.disallowed_tools, vec![ToolName::Edit.as_str()]);
    assert_eq!(def.max_turns, Some(15));
    assert!(matches!(
        def.identity,
        Some(cocode_protocol::execution::ExecutionIdentity::Role(
            cocode_protocol::model::ModelRole::Fast
        ))
    ));
    assert!(matches!(
        def.permission_mode,
        Some(cocode_protocol::PermissionMode::Bypass)
    ));
    assert!(!def.fork_context);
    assert_eq!(def.color.as_deref(), Some("cyan"));
    assert_eq!(
        def.critical_reminder.as_deref(),
        Some("CRITICAL: This is a custom read-only agent.")
    );
    assert_eq!(def.source, AgentSource::UserSettings);
}

#[test]
fn test_load_agent_from_file_minimal() {
    let dir = TempDir::new().unwrap();
    let content = "---\ndescription: Minimal agent\n---\n";
    write_agent_file(dir.path(), "minimal.md", content);

    let path = dir.path().join("minimal.md");
    let def = load_agent_from_file(&path, AgentSource::ProjectSettings).unwrap();

    assert_eq!(def.name, "minimal");
    assert_eq!(def.agent_type, "minimal");
    assert_eq!(def.description, "Minimal agent");
    assert!(def.tools.is_empty());
    assert!(def.disallowed_tools.is_empty());
    assert!(def.identity.is_none());
    assert!(def.max_turns.is_none());
    assert!(def.permission_mode.is_none());
    assert!(!def.fork_context);
    assert!(def.color.is_none());
    assert!(def.critical_reminder.is_none());
    assert_eq!(def.source, AgentSource::ProjectSettings);
}

#[test]
fn test_load_agents_from_dir() {
    let dir = TempDir::new().unwrap();
    write_agent_file(
        dir.path(),
        "agent-a.md",
        "---\nname: agent-a\ndescription: Agent A\n---\n",
    );
    write_agent_file(
        dir.path(),
        "agent-b.md",
        "---\nname: agent-b\ndescription: Agent B\n---\n",
    );
    // Non-md file should be ignored
    write_agent_file(dir.path(), "readme.txt", "not an agent");

    let agents = load_agents_from_dir(dir.path(), AgentSource::UserSettings);
    assert_eq!(agents.len(), 2);

    let types: Vec<&str> = agents.iter().map(|a| a.agent_type.as_str()).collect();
    assert!(types.contains(&"agent-a"));
    assert!(types.contains(&"agent-b"));
}

#[test]
fn test_load_agents_from_nonexistent_dir() {
    let agents = load_agents_from_dir(Path::new("/nonexistent/path"), AgentSource::UserSettings);
    assert!(agents.is_empty());
}

#[test]
fn test_merge_custom_agents_override() {
    let mut existing = vec![AgentDefinition {
        name: "explore".to_string(),
        description: "Original".to_string(),
        agent_type: "explore".to_string(),
        tools: vec![],
        disallowed_tools: vec![],
        identity: None,
        max_turns: Some(20),
        permission_mode: None,
        fork_context: false,
        color: None,
        critical_reminder: None,
        source: AgentSource::BuiltIn,
        skills: vec![],
        background: false,
        memory: None,
        hooks: None,
        mcp_servers: None,
        isolation: None,
        use_custom_prompt: false,
    }];

    let custom = vec![AgentDefinition {
        name: "explore".to_string(),
        description: "Custom explore".to_string(),
        agent_type: "explore".to_string(),
        tools: vec![ToolName::Read.as_str().to_string()],
        disallowed_tools: vec![],
        identity: None,
        max_turns: Some(50),
        permission_mode: None,
        fork_context: false,
        color: Some("green".to_string()),
        critical_reminder: None,
        source: AgentSource::UserSettings,
        skills: vec![],
        background: false,
        memory: None,
        hooks: None,
        mcp_servers: None,
        isolation: None,
        use_custom_prompt: false,
    }];

    merge_custom_agents(&mut existing, custom);
    assert_eq!(existing.len(), 1);
    assert_eq!(existing[0].description, "Custom explore");
    assert_eq!(existing[0].max_turns, Some(50));
    assert_eq!(existing[0].source, AgentSource::UserSettings);
}

#[test]
fn test_merge_custom_agents_new_type() {
    let mut existing = vec![AgentDefinition {
        name: "bash".to_string(),
        description: "Bash".to_string(),
        agent_type: "bash".to_string(),
        tools: vec![ToolName::Bash.as_str().to_string()],
        disallowed_tools: vec![],
        identity: None,
        max_turns: Some(10),
        permission_mode: None,
        fork_context: false,
        color: None,
        critical_reminder: None,
        source: AgentSource::BuiltIn,
        skills: vec![],
        background: false,
        memory: None,
        hooks: None,
        mcp_servers: None,
        isolation: None,
        use_custom_prompt: false,
    }];

    let custom = vec![AgentDefinition {
        name: "custom-agent".to_string(),
        description: "My custom agent".to_string(),
        agent_type: "custom-agent".to_string(),
        tools: vec![],
        disallowed_tools: vec![],
        identity: None,
        max_turns: None,
        permission_mode: None,
        fork_context: false,
        color: None,
        critical_reminder: None,
        source: AgentSource::ProjectSettings,
        skills: vec![],
        background: false,
        memory: None,
        hooks: None,
        mcp_servers: None,
        isolation: None,
        use_custom_prompt: false,
    }];

    merge_custom_agents(&mut existing, custom);
    assert_eq!(existing.len(), 2);
    assert_eq!(existing[1].agent_type, "custom-agent");
}

#[test]
fn test_load_custom_agents_integration() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();

    // Create user agent dir
    let user_dir = home.path().join("agents");
    std::fs::create_dir_all(&user_dir).unwrap();
    write_agent_file(
        &user_dir,
        "user-agent.md",
        "---\nname: user-agent\ndescription: User agent\n---\n",
    );

    // Create project agent dir
    let proj_dir = project.path().join(".cocode").join("agents");
    std::fs::create_dir_all(&proj_dir).unwrap();
    write_agent_file(
        &proj_dir,
        "proj-agent.md",
        "---\nname: proj-agent\ndescription: Project agent\n---\n",
    );

    let agents = load_custom_agents(home.path(), Some(project.path()));
    assert_eq!(agents.len(), 2);

    let types: Vec<&str> = agents.iter().map(|a| a.agent_type.as_str()).collect();
    assert!(types.contains(&"user-agent"));
    assert!(types.contains(&"proj-agent"));

    // User agent has UserSettings source
    let user = agents
        .iter()
        .find(|a| a.agent_type == "user-agent")
        .unwrap();
    assert_eq!(user.source, AgentSource::UserSettings);

    // Project agent has ProjectSettings source
    let proj = agents
        .iter()
        .find(|a| a.agent_type == "proj-agent")
        .unwrap();
    assert_eq!(proj.source, AgentSource::ProjectSettings);
}

// parse_identity tests moved to cocode_protocol::execution::identity.test.rs
// (ExecutionIdentity::parse_loose)

#[test]
fn test_parse_permission_mode_variants() {
    use cocode_protocol::PermissionMode;

    assert!(matches!(
        parse_permission_mode("bypass"),
        PermissionMode::Bypass
    ));
    assert!(matches!(
        parse_permission_mode("dontask"),
        PermissionMode::DontAsk
    ));
    assert!(matches!(
        parse_permission_mode("dont-ask"),
        PermissionMode::DontAsk
    ));
    assert!(matches!(
        parse_permission_mode("default"),
        PermissionMode::Default
    ));
    assert!(matches!(
        parse_permission_mode("plan"),
        PermissionMode::Plan
    ));
    assert!(matches!(
        parse_permission_mode("acceptEdits"),
        PermissionMode::AcceptEdits
    ));
    assert!(matches!(
        parse_permission_mode("accept-edits"),
        PermissionMode::AcceptEdits
    ));
    assert!(matches!(
        parse_permission_mode("anything"),
        PermissionMode::Default
    ));
}
