use super::*;

#[test]
fn test_deserialize_full() {
    let toml_str = r#"
name = "commit"
description = "Generate a commit message"
prompt_file = "prompt.md"
allowed_tools = ["Bash", "Read"]
"#;
    let iface: SkillInterface = toml::from_str(toml_str).expect("parse SKILL.toml");
    assert_eq!(iface.name, "commit");
    assert_eq!(iface.description, "Generate a commit message");
    assert_eq!(iface.prompt_file, Some("prompt.md".to_string()));
    assert!(iface.prompt_inline.is_none());
    assert_eq!(
        iface.allowed_tools,
        Some(vec!["Bash".to_string(), "Read".to_string()])
    );
}

#[test]
fn test_deserialize_inline_prompt() {
    let toml_str = r#"
name = "review"
description = "Review code"
prompt_inline = "Please review the following code changes."
"#;
    let iface: SkillInterface = toml::from_str(toml_str).expect("parse SKILL.toml");
    assert_eq!(iface.name, "review");
    assert_eq!(
        iface.prompt_inline,
        Some("Please review the following code changes.".to_string())
    );
    assert!(iface.prompt_file.is_none());
    assert!(iface.allowed_tools.is_none());
}

#[test]
fn test_deserialize_minimal() {
    let toml_str = r#"
name = "test"
description = "A test skill"
"#;
    let iface: SkillInterface = toml::from_str(toml_str).expect("parse SKILL.toml");
    assert_eq!(iface.name, "test");
    assert!(iface.prompt_file.is_none());
    assert!(iface.prompt_inline.is_none());
    assert!(iface.allowed_tools.is_none());
}

#[test]
fn test_deserialize_new_fields() {
    let toml_str = r#"
name = "deploy"
description = "Deploy to staging"
prompt_inline = "Deploy the app"
user_invocable = true
disable_model_invocation = true
model = "sonnet"
context = "fork"
agent = "deploy-agent"
argument_hint = "<environment>"
when_to_use = "When the user wants to deploy"
aliases = ["dep", "ship"]
"#;
    let iface: SkillInterface = toml::from_str(toml_str).expect("parse SKILL.toml");
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
        prompt_file: None,
        prompt_inline: Some("Do things".to_string()),
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
    let serialized = toml::to_string(&iface).expect("serialize");
    let deserialized: SkillInterface = toml::from_str(&serialized).expect("deserialize");
    assert_eq!(deserialized.name, "roundtrip");
    assert_eq!(deserialized.prompt_inline, Some("Do things".to_string()));
    assert_eq!(deserialized.allowed_tools, Some(vec!["Bash".to_string()]));
}

#[test]
fn test_deserialize_with_hooks() {
    let toml_str = r#"
name = "lint-check"
description = "Skill with hooks"
prompt_inline = "Do the thing"

[[hooks.PreToolUse]]
command = "npm run lint"
timeout_secs = 10
once = true

[hooks.PreToolUse.matcher]
type = "exact"
value = "Write"
"#;
    let iface: SkillInterface = toml::from_str(toml_str).expect("parse SKILL.toml");
    assert_eq!(iface.name, "lint-check");
    assert!(iface.hooks.is_some());

    let hooks = iface.hooks.unwrap();
    let pre_hooks = hooks.get("PreToolUse").expect("PreToolUse hooks");
    assert_eq!(pre_hooks.len(), 1);
    assert_eq!(pre_hooks[0].command, Some("npm run lint".to_string()));
    assert_eq!(pre_hooks[0].timeout_secs, 10);
    assert!(pre_hooks[0].once);
}

#[test]
fn test_deserialize_hook_or_matcher() {
    let toml_str = r#"
name = "multi-matcher"
description = "Skill with OR matcher"
prompt_inline = "test"

[[hooks.PreToolUse]]
command = "check"

[hooks.PreToolUse.matcher]
type = "or"

[[hooks.PreToolUse.matcher.matchers]]
type = "exact"
value = "Write"

[[hooks.PreToolUse.matcher.matchers]]
type = "exact"
value = "Edit"
"#;
    let iface: SkillInterface = toml::from_str(toml_str).expect("parse SKILL.toml");
    let hooks = iface.hooks.expect("hooks");
    let pre_hooks = hooks.get("PreToolUse").expect("PreToolUse");

    if let Some(SkillHookMatcher::Or { matchers }) = &pre_hooks[0].matcher {
        assert_eq!(matchers.len(), 2);
    } else {
        panic!("Expected Or matcher");
    }
}

#[test]
fn test_skill_hook_config_defaults() {
    let toml_str = r#"
name = "defaults"
description = "Test defaults"
prompt_inline = "test"

[[hooks.PostToolUse]]
command = "echo done"
"#;
    let iface: SkillInterface = toml::from_str(toml_str).expect("parse");
    let hooks = iface.hooks.expect("hooks");
    let post_hooks = hooks.get("PostToolUse").expect("PostToolUse");

    assert_eq!(post_hooks[0].timeout_secs, 30); // default
    assert!(!post_hooks[0].once); // default false
    assert!(post_hooks[0].matcher.is_none()); // no matcher
    assert!(post_hooks[0].args.is_none()); // no args
}
