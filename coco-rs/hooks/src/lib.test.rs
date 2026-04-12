use std::collections::HashMap;

use coco_types::HookEventType;
use coco_types::HookScope;

use super::*;

#[test]
fn test_hook_registry_register_and_find() {
    let mut registry = HookRegistry::new();
    registry.register(HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: Some("Bash".into()),
        handler: HookHandler::Command {
            command: "echo pre-hook".into(),
            timeout_ms: Some(5000),
            shell: None,
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });
    registry.register(HookDefinition {
        event: HookEventType::PostToolUse,
        matcher: None,
        handler: HookHandler::Command {
            command: "echo post-hook".into(),
            timeout_ms: None,
            shell: None,
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    assert_eq!(registry.len(), 2);

    // Find hooks for PreToolUse on "Bash"
    let matches = registry.find(HookEventType::PreToolUse, Some("Bash"));
    assert_eq!(matches.len(), 1);

    // Find hooks for PreToolUse on "Read" — no matcher match
    let matches = registry.find(HookEventType::PreToolUse, Some("Read"));
    assert_eq!(matches.len(), 0);

    // PostToolUse with no matcher matches everything
    let matches = registry.find(HookEventType::PostToolUse, Some("Bash"));
    assert_eq!(matches.len(), 1);
}

#[test]
fn test_hook_wildcard_matcher() {
    let mut registry = HookRegistry::new();
    registry.register(HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: Some("*".into()),
        handler: HookHandler::Command {
            command: "echo any".into(),
            timeout_ms: None,
            shell: None,
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    let matches = registry.find(HookEventType::PreToolUse, Some("anything"));
    assert_eq!(matches.len(), 1);
}

#[test]
fn test_hooks_settings_deserialization() {
    let json = r#"{"hooks": {"pre_tool_use": [{"event": "pre_tool_use", "matcher": "Bash", "handler": {"type": "command", "command": "echo hi"}}]}}"#;
    let settings: HooksSettings = serde_json::from_str(json).unwrap();
    assert!(settings.hooks.contains_key("pre_tool_use"));
}

#[test]
fn test_find_matching_returns_sorted_by_scope_then_priority() {
    let mut registry = HookRegistry::new();
    registry.register(HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Prompt {
            prompt: "low priority user".into(),
        },
        priority: 10,
        scope: HookScope::User,
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });
    registry.register(HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Prompt {
            prompt: "high priority user".into(),
        },
        priority: -5,
        scope: HookScope::User,
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });
    registry.register(HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Prompt {
            prompt: "session scope".into(),
        },
        priority: 100,
        scope: HookScope::Session,
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    let matches = registry.find_matching(HookEventType::PreToolUse, Some("Bash"));
    assert_eq!(matches.len(), 3);
    // Session scope first despite highest priority value
    assert_eq!(matches[0].scope, HookScope::Session);
    // Then User scope sorted by priority ascending
    assert_eq!(matches[1].priority, -5);
    assert_eq!(matches[2].priority, 10);
}

#[test]
fn test_glob_matcher() {
    let mut registry = HookRegistry::new();
    registry.register(HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: Some("Read*".into()),
        handler: HookHandler::Prompt {
            prompt: "matched".into(),
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    // Matches glob pattern
    let matches = registry.find_matching(HookEventType::PreToolUse, Some("ReadFile"));
    assert_eq!(matches.len(), 1);

    // Does not match
    let matches = registry.find_matching(HookEventType::PreToolUse, Some("Write"));
    assert_eq!(matches.len(), 0);
}

#[test]
fn test_pipe_separated_matcher() {
    let mut registry = HookRegistry::new();
    registry.register(HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: Some("Write|Edit|Bash".into()),
        handler: HookHandler::Prompt {
            prompt: "matched".into(),
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    assert_eq!(
        registry
            .find_matching(HookEventType::PreToolUse, Some("Write"))
            .len(),
        1
    );
    assert_eq!(
        registry
            .find_matching(HookEventType::PreToolUse, Some("Edit"))
            .len(),
        1
    );
    assert_eq!(
        registry
            .find_matching(HookEventType::PreToolUse, Some("Bash"))
            .len(),
        1
    );
    assert_eq!(
        registry
            .find_matching(HookEventType::PreToolUse, Some("Read"))
            .len(),
        0
    );
}

#[test]
fn test_regex_matcher() {
    let mut registry = HookRegistry::new();
    registry.register(HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: Some("^(Write|Edit)$".into()),
        handler: HookHandler::Prompt {
            prompt: "matched".into(),
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    assert_eq!(
        registry
            .find_matching(HookEventType::PreToolUse, Some("Write"))
            .len(),
        1
    );
    assert_eq!(
        registry
            .find_matching(HookEventType::PreToolUse, Some("Edit"))
            .len(),
        1
    );
    assert_eq!(
        registry
            .find_matching(HookEventType::PreToolUse, Some("WriteFile"))
            .len(),
        0
    );
}

#[test]
fn test_regex_prefix_matcher() {
    let mut registry = HookRegistry::new();
    registry.register(HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: Some("^Read.*".into()),
        handler: HookHandler::Prompt {
            prompt: "matched".into(),
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    assert_eq!(
        registry
            .find_matching(HookEventType::PreToolUse, Some("Read"))
            .len(),
        1
    );
    assert_eq!(
        registry
            .find_matching(HookEventType::PreToolUse, Some("ReadFile"))
            .len(),
        1
    );
    assert_eq!(
        registry
            .find_matching(HookEventType::PreToolUse, Some("Write"))
            .len(),
        0
    );
}

#[test]
fn test_prompt_handler() {
    let handler = HookHandler::Prompt {
        prompt: "Check for security issues".into(),
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = rt
        .block_on(execute_hook(&handler, &HashMap::new(), None))
        .unwrap();

    match result {
        HookExecutionResult::PromptText(text) => {
            assert_eq!(text, "Check for security issues");
        }
        HookExecutionResult::CommandOutput { .. } => {
            panic!("expected PromptText, got CommandOutput");
        }
    }
}

#[tokio::test]
async fn test_command_hook_execution() {
    let handler = HookHandler::Command {
        command: "echo hello-hook".into(),
        timeout_ms: Some(5000),
        shell: None,
    };

    let result = execute_hook(&handler, &HashMap::new(), None).await.unwrap();

    match result {
        HookExecutionResult::CommandOutput {
            exit_code,
            stdout,
            stderr: _,
        } => {
            assert_eq!(exit_code, 0);
            assert_eq!(stdout.trim(), "hello-hook");
        }
        HookExecutionResult::PromptText(_) => {
            panic!("expected CommandOutput, got PromptText");
        }
    }
}

#[tokio::test]
async fn test_execute_hooks_runs_all_matching() {
    let mut registry = HookRegistry::new();
    registry.register(HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Command {
            command: "echo first".into(),
            timeout_ms: None,
            shell: None,
        },
        priority: 1,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });
    registry.register(HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Prompt {
            prompt: "second".into(),
        },
        priority: 2,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });
    // Different event — should not match
    registry.register(HookDefinition {
        event: HookEventType::PostToolUse,
        matcher: None,
        handler: HookHandler::Prompt {
            prompt: "wrong event".into(),
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    let results = registry
        .execute_hooks(HookEventType::PreToolUse, Some("Bash"))
        .await;
    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    assert!(results[1].is_ok());
}

#[test]
fn test_no_tool_name_with_wildcard_does_not_match() {
    let mut registry = HookRegistry::new();
    registry.register(HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: Some("*".into()),
        handler: HookHandler::Prompt { prompt: "x".into() },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    // Wildcard requires a tool name to be present
    let matches = registry.find_matching(HookEventType::PreToolUse, None);
    assert_eq!(matches.len(), 0);
}

#[test]
fn test_no_matcher_matches_without_tool_name() {
    let mut registry = HookRegistry::new();
    registry.register(HookDefinition {
        event: HookEventType::SessionStart,
        matcher: None,
        handler: HookHandler::Command {
            command: "echo start".into(),
            timeout_ms: None,
            shell: None,
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    let matches = registry.find_matching(HookEventType::SessionStart, None);
    assert_eq!(matches.len(), 1);
}

#[test]
fn test_scope_ordering() {
    assert!(HookScope::Session > HookScope::Local);
    assert!(HookScope::Local > HookScope::Project);
    assert!(HookScope::Project > HookScope::User);
    assert!(HookScope::User > HookScope::Builtin);
}

#[test]
fn test_new_event_types_exist() {
    // Verify the 5 new event types are usable.
    let events = [
        HookEventType::NotebookCellExecute,
        HookEventType::ModelSwitch,
        HookEventType::ContextOverflow,
        HookEventType::BudgetWarning,
        HookEventType::QueryStart,
    ];
    for event in &events {
        let mut registry = HookRegistry::new();
        registry.register(HookDefinition {
            event: *event,
            matcher: None,
            handler: HookHandler::Prompt {
                prompt: "test".into(),
            },
            priority: 0,
            scope: HookScope::default(),
            if_condition: None,
            once: false,
            is_async: false,
            async_rewake: false,
            shell: None,
            status_message: None,
        });
        let matches = registry.find_matching(*event, None);
        assert_eq!(matches.len(), 1);
    }
}

#[test]
fn test_if_condition_filters_matching_hooks() {
    let mut registry = HookRegistry::new();
    // Hook with if_condition: only matches "Bash(git *)"
    registry.register(HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Prompt {
            prompt: "git-only".into(),
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: Some("Bash(git *)".into()),
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });
    // Hook without if_condition: matches everything
    registry.register(HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Prompt {
            prompt: "any".into(),
        },
        priority: 1,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    let ctx = IfConditionContext {
        tool_name: "Bash".to_string(),
        tool_content: Some("git status".to_string()),
    };

    // With if_condition context: both match (git hook matches "git status")
    let matches =
        registry.find_matching_with_if(HookEventType::PreToolUse, Some("Bash"), Some(&ctx));
    assert_eq!(matches.len(), 2);

    // Different content: only the unconditional hook matches
    let ctx2 = IfConditionContext {
        tool_name: "Bash".to_string(),
        tool_content: Some("npm install".to_string()),
    };
    let matches =
        registry.find_matching_with_if(HookEventType::PreToolUse, Some("Bash"), Some(&ctx2));
    assert_eq!(matches.len(), 1);

    // Without if_condition context: all hooks returned (no filtering)
    let matches = registry.find_matching_with_if(HookEventType::PreToolUse, Some("Bash"), None);
    assert_eq!(matches.len(), 2);
}

// -----------------------------------------------------------------------
// load_hooks_from_config tests
// -----------------------------------------------------------------------

#[test]
fn test_load_hooks_from_config_command() {
    let json = serde_json::json!({
        "pre_tool_use": [{
            "type": "command",
            "command": "echo hi",
            "matcher": "Bash"
        }]
    });

    let hooks = load_hooks_from_config(&json, HookScope::Project).unwrap();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].event, HookEventType::PreToolUse);
    assert_eq!(hooks[0].matcher.as_deref(), Some("Bash"));
    assert_eq!(hooks[0].scope, HookScope::Project);
    match &hooks[0].handler {
        HookHandler::Command { command, shell, .. } => {
            assert_eq!(command, "echo hi");
            assert!(shell.is_none());
        }
        other => panic!("expected Command, got {other:?}"),
    }
}

#[test]
fn test_load_hooks_from_config_prompt() {
    let json = serde_json::json!({
        "session_start": [{
            "type": "prompt",
            "prompt": "hello world"
        }]
    });

    let hooks = load_hooks_from_config(&json, HookScope::User).unwrap();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].event, HookEventType::SessionStart);
    assert!(hooks[0].matcher.is_none());
    match &hooks[0].handler {
        HookHandler::Prompt { prompt } => assert_eq!(prompt, "hello world"),
        other => panic!("expected Prompt, got {other:?}"),
    }
}

#[test]
fn test_load_hooks_from_config_http() {
    let json = serde_json::json!({
        "post_tool_use": [{
            "type": "webhook",
            "url": "https://example.com/hook",
            "method": "PUT",
            "headers": {"Authorization": "Bearer abc"}
        }]
    });

    let hooks = load_hooks_from_config(&json, HookScope::Local).unwrap();
    assert_eq!(hooks.len(), 1);
    match &hooks[0].handler {
        HookHandler::Http {
            url,
            method,
            headers,
            ..
        } => {
            assert_eq!(url, "https://example.com/hook");
            assert_eq!(method.as_deref(), Some("PUT"));
            let hdrs = headers.as_ref().unwrap();
            assert_eq!(hdrs.get("Authorization").unwrap(), "Bearer abc");
        }
        other => panic!("expected Http, got {other:?}"),
    }
}

#[test]
fn test_load_hooks_from_config_http_type_alias() {
    let json = serde_json::json!({
        "post_tool_use": [{
            "type": "http",
            "url": "https://example.com/hook2"
        }]
    });

    let hooks = load_hooks_from_config(&json, HookScope::User).unwrap();
    assert_eq!(hooks.len(), 1);
    match &hooks[0].handler {
        HookHandler::Http { url, .. } => assert_eq!(url, "https://example.com/hook2"),
        other => panic!("expected Http, got {other:?}"),
    }
}

#[test]
fn test_load_hooks_from_config_agent() {
    let json = serde_json::json!({
        "pre_tool_use": [{
            "type": "agent",
            "agent_name": "security-check",
            "prompt": "review this"
        }]
    });

    let hooks = load_hooks_from_config(&json, HookScope::Session).unwrap();
    assert_eq!(hooks.len(), 1);
    match &hooks[0].handler {
        HookHandler::Agent { agent_name, prompt } => {
            assert_eq!(agent_name, "security-check");
            assert_eq!(prompt.as_deref(), Some("review this"));
        }
        other => panic!("expected Agent, got {other:?}"),
    }
}

#[test]
fn test_load_hooks_from_config_with_if_condition() {
    let json = serde_json::json!({
        "pre_tool_use": [{
            "type": "command",
            "command": "echo check",
            "if": "Bash(git *)"
        }]
    });

    let hooks = load_hooks_from_config(&json, HookScope::User).unwrap();
    assert_eq!(hooks[0].if_condition.as_deref(), Some("Bash(git *)"));
}

#[test]
fn test_load_hooks_from_config_with_timeout_seconds() {
    let json = serde_json::json!({
        "pre_tool_use": [{
            "type": "command",
            "command": "echo slow",
            "timeout": 30
        }]
    });

    let hooks = load_hooks_from_config(&json, HookScope::User).unwrap();
    match &hooks[0].handler {
        HookHandler::Command { timeout_ms, .. } => {
            assert_eq!(*timeout_ms, Some(30_000));
        }
        other => panic!("expected Command, got {other:?}"),
    }
}

#[test]
fn test_load_hooks_from_config_timeout_does_not_override_handler_timeout() {
    let json = serde_json::json!({
        "pre_tool_use": [{
            "type": "command",
            "command": "echo fast",
            "timeout_ms": 5000,
            "timeout": 60
        }]
    });

    let hooks = load_hooks_from_config(&json, HookScope::User).unwrap();
    match &hooks[0].handler {
        HookHandler::Command { timeout_ms, .. } => {
            // Handler-level timeout_ms (5000) should be preserved, not overridden
            assert_eq!(*timeout_ms, Some(5000));
        }
        other => panic!("expected Command, got {other:?}"),
    }
}

#[test]
fn test_load_hooks_from_config_with_optional_fields() {
    let json = serde_json::json!({
        "pre_tool_use": [{
            "type": "command",
            "command": "echo hi",
            "once": true,
            "async": true,
            "async_rewake": true,
            "shell": "bash",
            "status_message": "Running check..."
        }]
    });

    let hooks = load_hooks_from_config(&json, HookScope::User).unwrap();
    assert!(hooks[0].once);
    assert!(hooks[0].is_async);
    assert!(hooks[0].async_rewake);
    assert_eq!(hooks[0].shell.as_deref(), Some("bash"));
    assert_eq!(hooks[0].status_message.as_deref(), Some("Running check..."));
}

#[test]
fn test_load_hooks_from_config_matcher_with_tool_name_object() {
    let json = serde_json::json!({
        "pre_tool_use": [{
            "type": "command",
            "command": "echo hi",
            "matcher": {"tool_name": "Write"}
        }]
    });

    let hooks = load_hooks_from_config(&json, HookScope::User).unwrap();
    assert_eq!(hooks[0].matcher.as_deref(), Some("Write"));
}

#[test]
fn test_load_hooks_from_config_multiple_events() {
    let json = serde_json::json!({
        "pre_tool_use": [
            {"type": "command", "command": "echo a"},
            {"type": "command", "command": "echo b"}
        ],
        "session_start": [
            {"type": "prompt", "prompt": "welcome"}
        ]
    });

    let hooks = load_hooks_from_config(&json, HookScope::User).unwrap();
    assert_eq!(hooks.len(), 3);
}

#[test]
fn test_load_hooks_from_config_unknown_event_type_errors() {
    let json = serde_json::json!({
        "bogus_event": [{"type": "command", "command": "echo x"}]
    });

    let result = load_hooks_from_config(&json, HookScope::User);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("bogus_event"));
}

#[test]
fn test_load_hooks_from_config_unknown_handler_type_errors() {
    let json = serde_json::json!({
        "pre_tool_use": [{"type": "ftp", "url": "ftp://x"}]
    });

    let result = load_hooks_from_config(&json, HookScope::User);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("ftp"));
}

#[test]
fn test_load_hooks_from_config_not_object_errors() {
    let json = serde_json::json!("not an object");
    let result = load_hooks_from_config(&json, HookScope::User);
    assert!(result.is_err());
}

// -----------------------------------------------------------------------
// register_deduped tests
// -----------------------------------------------------------------------

#[test]
fn test_register_deduped_allows_unique_hooks() {
    let mut registry = HookRegistry::new();
    let h1 = HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Command {
            command: "echo a".into(),
            timeout_ms: None,
            shell: None,
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    };
    let h2 = HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Command {
            command: "echo b".into(),
            timeout_ms: None,
            shell: None,
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    };

    assert!(registry.register_deduped(h1));
    assert!(registry.register_deduped(h2));
    assert_eq!(registry.len(), 2);
}

#[test]
fn test_register_deduped_rejects_duplicate_command() {
    let mut registry = HookRegistry::new();
    let h1 = HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Command {
            command: "echo same".into(),
            timeout_ms: None,
            shell: None,
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    };
    let h2 = HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: Some("Bash".into()),
        handler: HookHandler::Command {
            command: "echo same".into(),
            timeout_ms: Some(5000),
            shell: None,
        },
        priority: 10,
        scope: HookScope::Session,
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    };

    assert!(registry.register_deduped(h1));
    // Same command + same shell + same if_condition = duplicate
    assert!(!registry.register_deduped(h2));
    assert_eq!(registry.len(), 1);
}

#[test]
fn test_register_deduped_different_if_condition_not_duplicate() {
    let mut registry = HookRegistry::new();
    let h1 = HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Command {
            command: "echo check".into(),
            timeout_ms: None,
            shell: None,
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    };
    let h2 = HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Command {
            command: "echo check".into(),
            timeout_ms: None,
            shell: None,
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: Some("Bash(git *)".into()),
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    };

    assert!(registry.register_deduped(h1));
    assert!(registry.register_deduped(h2));
    assert_eq!(registry.len(), 2);
}

#[test]
fn test_register_deduped_different_shell_not_duplicate() {
    let mut registry = HookRegistry::new();
    let h1 = HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Command {
            command: "echo check".into(),
            timeout_ms: None,
            shell: None,
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    };
    let h2 = HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Command {
            command: "echo check".into(),
            timeout_ms: None,
            shell: Some("bash".into()),
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    };

    assert!(registry.register_deduped(h1));
    assert!(registry.register_deduped(h2));
    assert_eq!(registry.len(), 2);
}

#[test]
fn test_register_deduped_prompt_hooks() {
    let mut registry = HookRegistry::new();
    let h1 = HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Prompt {
            prompt: "same prompt".into(),
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    };
    let h2 = h1.clone();

    assert!(registry.register_deduped(h1));
    assert!(!registry.register_deduped(h2));
    assert_eq!(registry.len(), 1);
}

#[tokio::test]
async fn test_command_hook_custom_shell() {
    let handler = HookHandler::Command {
        command: "echo custom-shell".into(),
        timeout_ms: Some(5000),
        shell: Some("bash".into()),
    };

    let result = execute_hook(&handler, &HashMap::new(), None).await.unwrap();

    match result {
        HookExecutionResult::CommandOutput {
            exit_code, stdout, ..
        } => {
            assert_eq!(exit_code, 0);
            assert_eq!(stdout.trim(), "custom-shell");
        }
        HookExecutionResult::PromptText(_) => {
            panic!("expected CommandOutput, got PromptText");
        }
    }
}

// -----------------------------------------------------------------------
// matcher_matches — TS heuristic alignment tests
// -----------------------------------------------------------------------

#[test]
fn test_matcher_simple_alphanumeric_only() {
    // Pattern with only alphanumeric + underscore → exact match
    assert!(matcher_matches(Some("Read"), Some("Read")));
    assert!(!matcher_matches(Some("Read"), Some("Write")));
}

#[test]
fn test_matcher_regex_with_dot_star() {
    // "Read.*" contains '.' which is not simple → treated as regex
    assert!(matcher_matches(Some("Read.*"), Some("ReadFile")));
    assert!(matcher_matches(Some("Read.*"), Some("Read")));
    assert!(!matcher_matches(Some("Read.*"), Some("Write")));
}

#[test]
fn test_matcher_regex_dot_plus() {
    // "Read.+" is regex, not glob
    assert!(matcher_matches(Some("Read.+"), Some("ReadFile")));
    assert!(!matcher_matches(Some("Read.+"), Some("Read"))); // .+ needs at least one char
}

#[test]
fn test_matcher_pipe_with_special_chars_is_regex() {
    // "Write|Edit|Read.*" has '.' → treated as regex, not pipe-separated
    assert!(matcher_matches(Some("Write|Edit|Read.*"), Some("Write")));
    assert!(matcher_matches(Some("Write|Edit|Read.*"), Some("Edit")));
    assert!(matcher_matches(Some("Write|Edit|Read.*"), Some("ReadFile")));
}

#[test]
fn test_matcher_glob_fallback_on_invalid_regex() {
    // Invalid regex like "[" falls through to glob
    // "[" is also invalid glob, so should return false
    assert!(!matcher_matches(Some("["), Some("anything")));
}

// -----------------------------------------------------------------------
// sanitize_header_value tests
// -----------------------------------------------------------------------

#[test]
fn test_sanitize_header_value_strips_crlf() {
    assert_eq!(sanitize_header_value("normal"), "normal");
    assert_eq!(sanitize_header_value("a\r\nb"), "ab");
    assert_eq!(sanitize_header_value("a\0b"), "ab");
    assert_eq!(
        sanitize_header_value("token\r\nX-Evil: 1"),
        "tokenX-Evil: 1"
    );
}

// -----------------------------------------------------------------------
// interpolate_env_vars tests
// -----------------------------------------------------------------------

#[test]
fn test_interpolate_env_vars_from_hook_env() {
    let mut env = HashMap::new();
    env.insert("MY_TOKEN".to_string(), "secret123".to_string());

    assert_eq!(
        interpolate_env_vars("Bearer $MY_TOKEN", &env),
        "Bearer secret123"
    );
    assert_eq!(
        interpolate_env_vars("Bearer ${MY_TOKEN}", &env),
        "Bearer secret123"
    );
}

#[test]
fn test_interpolate_env_vars_missing_resolves_empty() {
    let env = HashMap::new();
    assert_eq!(
        interpolate_env_vars("Bearer $NONEXISTENT_VAR_XYZ", &env),
        "Bearer "
    );
}

#[test]
fn test_interpolate_env_vars_no_vars() {
    let env = HashMap::new();
    assert_eq!(interpolate_env_vars("no vars here", &env), "no vars here");
}

// -----------------------------------------------------------------------
// prompt request types tests
// -----------------------------------------------------------------------

#[test]
fn test_prompt_request_deserialize() {
    let json = r#"{"prompt": "req-1", "message": "Pick one:", "options": [{"key": "a", "label": "Option A"}, {"key": "b", "label": "Option B", "description": "Second option"}]}"#;
    let req: PromptRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.prompt, "req-1");
    assert_eq!(req.options.len(), 2);
    assert_eq!(req.options[1].description.as_deref(), Some("Second option"));
}

#[test]
fn test_prompt_response_serialize() {
    let resp = PromptResponse {
        prompt_response: "req-1".to_string(),
        selected: "a".to_string(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("prompt_response"));
    assert!(json.contains("\"a\""));
}
