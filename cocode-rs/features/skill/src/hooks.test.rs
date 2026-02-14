use super::*;
use std::collections::HashMap;

fn make_interface_with_hooks(hooks: HashMap<String, Vec<SkillHookConfig>>) -> SkillInterface {
    SkillInterface {
        name: "test-skill".to_string(),
        description: "A test skill".to_string(),
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
fn test_convert_skill_hooks_single_with_string_matcher() {
    let mut hooks = HashMap::new();
    hooks.insert(
        "PreToolUse".to_string(),
        vec![SkillHookConfig {
            matcher: Some("Write".to_string()),
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
fn test_convert_string_matcher_pipe_separated() {
    let matcher = convert_string_matcher("Write|Edit");

    if let HookMatcher::Or { matchers } = matcher {
        assert_eq!(matchers.len(), 2);
        assert!(matches!(&matchers[0], HookMatcher::Exact { value } if value == "Write"));
        assert!(matches!(&matchers[1], HookMatcher::Exact { value } if value == "Edit"));
    } else {
        panic!("Expected Or matcher");
    }
}

#[test]
fn test_convert_string_matcher_wildcard() {
    let matcher = convert_string_matcher("Bash*");
    assert!(matches!(matcher, HookMatcher::Wildcard { pattern } if pattern == "Bash*"));
}

#[test]
fn test_convert_string_matcher_exact() {
    let matcher = convert_string_matcher("Write");
    assert!(matches!(matcher, HookMatcher::Exact { value } if value == "Write"));
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
