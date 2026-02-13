use super::*;

#[test]
fn test_builtin_agent_override_defaults() {
    let override_cfg = BuiltinAgentOverride::default();
    assert!(override_cfg.max_turns.is_none());
    assert!(override_cfg.identity.is_none());
    assert!(override_cfg.tools.is_none());
    assert!(override_cfg.disallowed_tools.is_none());
}

#[test]
fn test_parse_config_json() {
    let json = r#"{
        "explore": {
            "max_turns": 30,
            "identity": "fast",
            "tools": ["Read", "Glob", "Grep"]
        },
        "plan": {
            "max_turns": 100
        }
    }"#;

    let config: BuiltinAgentsConfig = serde_json::from_str(json).expect("parse");

    let explore = config.get("explore").expect("explore config");
    assert_eq!(explore.max_turns, Some(30));
    assert_eq!(explore.identity.as_deref(), Some("fast"));
    assert_eq!(
        explore.tools.as_deref(),
        Some(&["Read".to_string(), "Glob".to_string(), "Grep".to_string()][..])
    );
    assert!(explore.disallowed_tools.is_none());

    let plan = config.get("plan").expect("plan config");
    assert_eq!(plan.max_turns, Some(100));
    assert!(plan.identity.is_none());
}

#[test]
fn test_parse_empty_config() {
    let json = "{}";
    let config: BuiltinAgentsConfig = serde_json::from_str(json).expect("parse");
    assert!(config.is_empty());
}

#[test]
fn test_serialize_config() {
    let mut config = BuiltinAgentsConfig::new();
    config.insert(
        "explore".to_string(),
        BuiltinAgentOverride {
            max_turns: Some(50),
            identity: Some("fast".to_string()),
            tools: None,
            disallowed_tools: None,
        },
    );

    let json = serde_json::to_string_pretty(&config).expect("serialize");
    assert!(json.contains("\"max_turns\": 50"));
    assert!(json.contains("\"identity\": \"fast\""));
    // Optional None fields should be skipped
    assert!(!json.contains("tools"));
    assert!(!json.contains("disallowed_tools"));
}

#[test]
fn test_is_builtin_agent() {
    assert!(is_builtin_agent("bash"));
    assert!(is_builtin_agent("explore"));
    assert!(is_builtin_agent("plan"));
    assert!(is_builtin_agent("general"));
    assert!(is_builtin_agent("guide"));
    assert!(is_builtin_agent("statusline"));

    assert!(!is_builtin_agent("custom"));
    assert!(!is_builtin_agent(""));
}

#[test]
fn test_load_nonexistent_file() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let config = load_builtin_agents_config(tmp.path());
    // Should return empty map, not error
    assert!(config.is_empty());
}
