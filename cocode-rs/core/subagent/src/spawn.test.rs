use super::*;
use cocode_protocol::ToolName;
use cocode_protocol::model::ModelRole;
use cocode_protocol::model::ModelSpec;

#[test]
fn test_spawn_input_defaults() {
    let json = r#"{"agent_type":"bash","prompt":"list files"}"#;
    let input: SpawnInput = serde_json::from_str(json).expect("deserialize");
    assert_eq!(input.agent_type, "bash");
    assert_eq!(input.prompt, "list files");
    assert!(input.identity.is_none());
    assert!(input.max_turns.is_none());
    assert!(input.run_in_background.is_none());
    assert!(input.allowed_tools.is_none());
    assert!(input.description.is_none());
}

#[test]
fn test_spawn_input_with_identity() {
    let input = SpawnInput {
        agent_type: "explore".to_string(),
        prompt: "find all tests".to_string(),
        identity: Some(ExecutionIdentity::Role(ModelRole::Explore)),
        max_turns: Some(20),
        run_in_background: Some(true),
        allowed_tools: Some(vec![
            ToolName::Read.as_str().to_string(),
            ToolName::Glob.as_str().to_string(),
        ]),
        resume_from: None,
        name: None,
        team_name: None,
        mode: None,
        cwd: None,
        isolation_override: None,
        description: None,
    };
    let json = serde_json::to_string(&input).expect("serialize");
    let back: SpawnInput = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.agent_type, "explore");
    assert_eq!(back.run_in_background, Some(true));
    assert!(matches!(
        back.identity,
        Some(ExecutionIdentity::Role(ModelRole::Explore))
    ));
}

#[test]
fn test_spawn_input_inherit_identity() {
    let input = SpawnInput {
        agent_type: "bash".to_string(),
        prompt: "test".to_string(),
        identity: Some(ExecutionIdentity::Inherit),
        max_turns: None,
        run_in_background: Some(false),
        allowed_tools: None,
        resume_from: None,
        name: None,
        team_name: None,
        mode: None,
        cwd: None,
        isolation_override: None,
        description: None,
    };
    assert!(matches!(input.identity, Some(ExecutionIdentity::Inherit)));
}

#[test]
fn test_spawn_input_spec_identity() {
    let spec = ModelSpec::new("anthropic", "claude-opus-4");
    let input = SpawnInput {
        agent_type: "general".to_string(),
        prompt: "test".to_string(),
        identity: Some(ExecutionIdentity::Spec(spec.clone())),
        max_turns: None,
        run_in_background: None,
        allowed_tools: None,
        resume_from: None,
        name: None,
        team_name: None,
        mode: None,
        cwd: None,
        isolation_override: None,
        description: None,
    };
    if let Some(ExecutionIdentity::Spec(s)) = &input.identity {
        assert_eq!(s, &spec);
    } else {
        panic!("Expected Spec identity");
    }
}

#[test]
fn test_spawn_input_new_fields_from_json() {
    let json = r#"{
        "agent_type": "explore",
        "prompt": "search",
        "name": "my-explorer",
        "team_name": "team-alpha",
        "mode": "plan",
        "cwd": "/tmp/work",
        "isolation_override": "worktree",
        "description": "Search for config files"
    }"#;
    let input: SpawnInput = serde_json::from_str(json).expect("deserialize");
    assert_eq!(input.name.as_deref(), Some("my-explorer"));
    assert_eq!(input.team_name.as_deref(), Some("team-alpha"));
    assert_eq!(input.mode.as_deref(), Some("plan"));
    assert_eq!(input.cwd.as_deref(), Some("/tmp/work"));
    assert_eq!(input.isolation_override.as_deref(), Some("worktree"));
    assert_eq!(
        input.description.as_deref(),
        Some("Search for config files")
    );
}

#[test]
fn test_spawn_input_new_fields_default_to_none() {
    let json = r#"{"agent_type":"bash","prompt":"test"}"#;
    let input: SpawnInput = serde_json::from_str(json).expect("deserialize");
    assert!(input.name.is_none());
    assert!(input.team_name.is_none());
    assert!(input.mode.is_none());
    assert!(input.cwd.is_none());
    assert!(input.isolation_override.is_none());
    assert!(input.description.is_none());
}

#[test]
fn test_spawn_input_new_fields_roundtrip() {
    let input = SpawnInput {
        agent_type: "general".to_string(),
        prompt: "do stuff".to_string(),
        identity: None,
        max_turns: None,
        run_in_background: None,
        allowed_tools: None,
        resume_from: None,
        name: Some("my-agent".to_string()),
        team_name: Some("ops".to_string()),
        mode: Some("auto".to_string()),
        cwd: Some("/home/user".to_string()),
        isolation_override: Some("worktree".to_string()),
        description: Some("Do various tasks".to_string()),
    };
    let json = serde_json::to_string(&input).expect("serialize");
    let back: SpawnInput = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.name, input.name);
    assert_eq!(back.team_name, input.team_name);
    assert_eq!(back.mode, input.mode);
    assert_eq!(back.cwd, input.cwd);
    assert_eq!(back.isolation_override, input.isolation_override);
    assert_eq!(back.description, input.description);
}

#[test]
fn test_spawn_input_description() {
    let json =
        r#"{"agent_type":"explore","prompt":"find stuff","description":"Search the codebase"}"#;
    let input: SpawnInput = serde_json::from_str(json).expect("deserialize");
    assert_eq!(input.description.as_deref(), Some("Search the codebase"));
}
