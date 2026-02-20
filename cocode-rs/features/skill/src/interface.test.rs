use super::*;

#[test]
fn test_deserialize_full() {
    let yaml = r#"
name: commit
description: Generate a commit message
allowed-tools:
  - Bash
  - Read
"#;
    let iface: SkillInterface = serde_yml::from_str(yaml).expect("parse YAML");
    assert_eq!(iface.name, "commit");
    assert_eq!(iface.description, "Generate a commit message");
    assert_eq!(
        iface.allowed_tools,
        Some(vec!["Bash".to_string(), "Read".to_string()])
    );
}

#[test]
fn test_deserialize_minimal() {
    let yaml = r#"
name: test
description: A test skill
"#;
    let iface: SkillInterface = serde_yml::from_str(yaml).expect("parse YAML");
    assert_eq!(iface.name, "test");
    assert!(iface.allowed_tools.is_none());
}

#[test]
fn test_deserialize_new_fields() {
    let yaml = r#"
name: deploy
description: Deploy to staging
user-invocable: true
disable-model-invocation: true
model: sonnet
context: fork
agent: deploy-agent
argument-hint: "<environment>"
when-to-use: When the user wants to deploy
aliases:
  - dep
  - ship
"#;
    let iface: SkillInterface = serde_yml::from_str(yaml).expect("parse YAML");
    assert_eq!(iface.name, "deploy");
    assert_eq!(iface.user_invocable, Some(true));
    assert_eq!(iface.disable_model_invocation, Some(true));
    assert_eq!(iface.model, Some("sonnet".to_string()));
    assert_eq!(iface.context, Some("fork".to_string()));
    assert_eq!(iface.agent, Some("deploy-agent".to_string()));
    assert_eq!(iface.argument_hint, Some("<environment>".to_string()));
    assert_eq!(
        iface.when_to_use,
        Some("When the user wants to deploy".to_string())
    );
    assert_eq!(
        iface.aliases,
        Some(vec!["dep".to_string(), "ship".to_string()])
    );
}

#[test]
fn test_serialize_roundtrip() {
    let iface = SkillInterface {
        name: "roundtrip".to_string(),
        description: "Roundtrip test".to_string(),
        allowed_tools: Some(vec!["Bash".to_string()]),
        when_to_use: None,
        user_invocable: None,
        disable_model_invocation: None,
        model: None,
        context: None,
        agent: None,
        argument_hint: None,
        aliases: None,
        hooks: None,
    };
    let serialized = serde_yml::to_string(&iface).expect("serialize");
    let deserialized: SkillInterface = serde_yml::from_str(&serialized).expect("deserialize");
    assert_eq!(deserialized.name, "roundtrip");
    assert_eq!(deserialized.allowed_tools, Some(vec!["Bash".to_string()]));
}

#[test]
fn test_deserialize_with_hooks() {
    let yaml = r#"
name: lint-check
description: Skill with hooks
hooks:
  PreToolUse:
    - command: npm run lint
      timeout_secs: 10
      once: true
      matcher: "Write"
"#;
    let iface: SkillInterface = serde_yml::from_str(yaml).expect("parse YAML");
    assert_eq!(iface.name, "lint-check");
    assert!(iface.hooks.is_some());

    let hooks = iface.hooks.unwrap();
    let pre_hooks = hooks.get("PreToolUse").expect("PreToolUse hooks");
    assert_eq!(pre_hooks.len(), 1);
    assert_eq!(pre_hooks[0].command, Some("npm run lint".to_string()));
    assert_eq!(pre_hooks[0].timeout_secs, 10);
    assert!(pre_hooks[0].once);
    assert_eq!(pre_hooks[0].matcher, Some("Write".to_string()));
}

#[test]
fn test_deserialize_hook_pipe_matcher() {
    let yaml = r#"
name: multi-matcher
description: Skill with pipe matcher
hooks:
  PreToolUse:
    - command: check
      matcher: "Write|Edit"
"#;
    let iface: SkillInterface = serde_yml::from_str(yaml).expect("parse YAML");
    let hooks = iface.hooks.expect("hooks");
    let pre_hooks = hooks.get("PreToolUse").expect("PreToolUse");

    assert_eq!(pre_hooks[0].matcher, Some("Write|Edit".to_string()));
}

#[test]
fn test_skill_hook_config_defaults() {
    let yaml = r#"
name: defaults
description: Test defaults
hooks:
  PostToolUse:
    - command: echo done
"#;
    let iface: SkillInterface = serde_yml::from_str(yaml).expect("parse");
    let hooks = iface.hooks.expect("hooks");
    let post_hooks = hooks.get("PostToolUse").expect("PostToolUse");

    assert_eq!(post_hooks[0].timeout_secs, 30); // default
    assert!(!post_hooks[0].once); // default false
    assert!(post_hooks[0].matcher.is_none()); // no matcher
    assert!(post_hooks[0].args.is_none()); // no args
}
