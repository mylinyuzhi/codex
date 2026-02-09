use super::*;
use std::collections::HashMap;

fn make_interface_with_hooks(hooks: HashMap<String, Vec<SkillHookConfig>>) -> SkillInterface {
    SkillInterface {
        name: "test-skill".to_string(),
        description: "A test skill".to_string(),
        prompt_file: None,
        prompt_inline: Some("test prompt".to_string()),
        allowed_tools: None,
        when_to_use: None,
        user_invocable: None,
        disable_model_invocation: None,
        model: None,
        context: None,
        agent: None,
        argument_hint: None,
        aliases: None,
        hooks: Some(hooks),
    }
}

#[test]
fn test_parse_event_type_pascal_case() {
    assert_eq!(
        parse_event_type("PreToolUse"),
        Some(HookEventType::PreToolUse)
    );
    assert_eq!(
        parse_event_type("PostToolUse"),
        Some(HookEventType::PostToolUse)
    );
    assert_eq!(
        parse_event_type("SessionStart"),
        Some(HookEventType::SessionStart)
    );
}

#[test]
fn test_parse_event_type_snake_case() {
    assert_eq!(
        parse_event_type("pre_tool_use"),
        Some(HookEventType::PreToolUse)
    );
    assert_eq!(
        parse_event_type("post_tool_use"),
        Some(HookEventType::PostToolUse)
    );
    assert_eq!(
        parse_event_type("session_start"),
        Some(HookEventType::SessionStart)
    );
}

#[test]
fn test_parse_event_type_unknown() {
    assert_eq!(parse_event_type("unknown_event"), None);
    assert_eq!(parse_event_type(""), None);
}

#[test]
fn test_convert_skill_hooks_empty() {
    let interface = SkillInterface {
        name: "test".to_string(),
        description: "Test".to_string(),
        prompt_file: None,
        prompt_inline: Some("test".to_string()),
        allowed_tools: None,
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
    let defs = convert_skill_hooks(&interface);
    assert!(defs.is_empty());
}

#[test]
fn test_convert_skill_hooks_single() {
    let mut hooks = HashMap::new();
    hooks.insert(
        "PreToolUse".to_string(),
        vec![SkillHookConfig {
            matcher: Some(SkillHookMatcher::Exact {
                value: "Write".to_string(),
            }),
            command: Some("npm run lint".to_string()),
            args: Some(vec!["--fix".to_string()]),
            timeout_secs: 60,
            once: true,
        }],
    );

    let interface = make_interface_with_hooks(hooks);
    let defs = convert_skill_hooks(&interface);

    assert_eq!(defs.len(), 1);
    let def = &defs[0];
    assert_eq!(def.name, "test-skill:hook:0");
    assert_eq!(def.event_type, HookEventType::PreToolUse);
    assert!(def.once);
    assert_eq!(def.timeout_secs, 60);

    if let HookHandler::Command { command, args } = &def.handler {
        assert_eq!(command, "npm run lint");
        assert_eq!(args, &vec!["--fix".to_string()]);
    } else {
        panic!("Expected Command handler");
    }

    if let Some(HookMatcher::Exact { value }) = &def.matcher {
        assert_eq!(value, "Write");
    } else {
        panic!("Expected Exact matcher");
    }

    if let HookSource::Skill { name } = &def.source {
        assert_eq!(name, "test-skill");
    } else {
        panic!("Expected Skill source");
    }
}

#[test]
fn test_convert_skill_hooks_multiple() {
    let mut hooks = HashMap::new();
    hooks.insert(
        "PreToolUse".to_string(),
        vec![
            SkillHookConfig {
                matcher: None,
                command: Some("echo pre".to_string()),
                args: None,
                timeout_secs: 30,
                once: false,
            },
            SkillHookConfig {
                matcher: None,
                command: Some("echo pre2".to_string()),
                args: None,
                timeout_secs: 30,
                once: false,
            },
        ],
    );
    hooks.insert(
        "PostToolUse".to_string(),
        vec![SkillHookConfig {
            matcher: None,
            command: Some("echo post".to_string()),
            args: None,
            timeout_secs: 30,
            once: false,
        }],
    );

    let interface = make_interface_with_hooks(hooks);
    let defs = convert_skill_hooks(&interface);

    assert_eq!(defs.len(), 3);
}

#[test]
fn test_convert_matcher_or() {
    let skill_matcher = SkillHookMatcher::Or {
        matchers: vec![
            SkillHookMatcher::Exact {
                value: "Write".to_string(),
            },
            SkillHookMatcher::Exact {
                value: "Edit".to_string(),
            },
        ],
    };

    let hook_matcher = convert_matcher(&skill_matcher);

    if let HookMatcher::Or { matchers } = hook_matcher {
        assert_eq!(matchers.len(), 2);
    } else {
        panic!("Expected Or matcher");
    }
}

#[test]
fn test_convert_matcher_all() {
    let skill_matcher = SkillHookMatcher::All;
    let hook_matcher = convert_matcher(&skill_matcher);
    assert!(matches!(hook_matcher, HookMatcher::All));
}

#[test]
fn test_convert_skill_hooks_skips_no_command() {
    let mut hooks = HashMap::new();
    hooks.insert(
        "PreToolUse".to_string(),
        vec![SkillHookConfig {
            matcher: None,
            command: None, // No command
            args: None,
            timeout_secs: 30,
            once: false,
        }],
    );

    let interface = make_interface_with_hooks(hooks);
    let defs = convert_skill_hooks(&interface);

    assert!(defs.is_empty());
}

#[test]
fn test_convert_skill_hooks_skips_unknown_event() {
    let mut hooks = HashMap::new();
    hooks.insert(
        "UnknownEvent".to_string(),
        vec![SkillHookConfig {
            matcher: None,
            command: Some("echo".to_string()),
            args: None,
            timeout_secs: 30,
            once: false,
        }],
    );

    let interface = make_interface_with_hooks(hooks);
    let defs = convert_skill_hooks(&interface);

    assert!(defs.is_empty());
}

#[test]
fn test_register_and_cleanup_skill_hooks() {
    let registry = HookRegistry::new();

    let mut hooks = HashMap::new();
    hooks.insert(
        "PreToolUse".to_string(),
        vec![SkillHookConfig {
            matcher: None,
            command: Some("echo test".to_string()),
            args: None,
            timeout_secs: 30,
            once: false,
        }],
    );

    let interface = make_interface_with_hooks(hooks);

    // Register
    let count = register_skill_hooks(&registry, &interface);
    assert_eq!(count, 1);

    // Verify registered
    let all = registry.all_hooks();
    assert_eq!(all.len(), 1);

    // Cleanup
    cleanup_skill_hooks(&registry, "test-skill");

    // Verify removed
    let all = registry.all_hooks();
    assert!(all.is_empty());
}
