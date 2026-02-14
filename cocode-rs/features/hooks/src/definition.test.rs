use super::*;

#[test]
fn test_hook_definition_defaults() {
    let json = r#"{
        "name": "test-hook",
        "event_type": "pre_tool_use",
        "handler": { "type": "command", "command": "echo", "args": ["hello"] }
    }"#;
    let def: HookDefinition = serde_json::from_str(json).expect("parse");
    assert_eq!(def.name, "test-hook");
    assert!(def.enabled);
    assert_eq!(def.timeout_secs, 30);
    assert!(def.matcher.is_none());
    // Source defaults to Session
    assert_eq!(def.source, HookSource::Session);
}

#[test]
fn test_hook_definition_with_source() {
    let json = r#"{
        "name": "policy-hook",
        "event_type": "pre_tool_use",
        "handler": { "type": "command", "command": "echo", "args": [] },
        "source": { "type": "policy" }
    }"#;
    let def: HookDefinition = serde_json::from_str(json).expect("parse");
    assert_eq!(def.source, HookSource::Policy);

    let json = r#"{
        "name": "plugin-hook",
        "event_type": "pre_tool_use",
        "handler": { "type": "command", "command": "echo", "args": [] },
        "source": { "type": "plugin", "name": "my-plugin" }
    }"#;
    let def: HookDefinition = serde_json::from_str(json).expect("parse");
    assert_eq!(
        def.source,
        HookSource::Plugin {
            name: "my-plugin".to_string()
        }
    );
}

#[test]
fn test_hook_definition_once_flag() {
    // Default is false
    let json = r#"{
        "name": "regular-hook",
        "event_type": "pre_tool_use",
        "handler": { "type": "command", "command": "echo", "args": [] }
    }"#;
    let def: HookDefinition = serde_json::from_str(json).expect("parse");
    assert!(!def.once);

    // Explicit true
    let json = r#"{
        "name": "one-shot-hook",
        "event_type": "pre_tool_use",
        "handler": { "type": "command", "command": "echo", "args": [] },
        "once": true
    }"#;
    let def: HookDefinition = serde_json::from_str(json).expect("parse");
    assert!(def.once);
}

#[test]
fn test_handler_command_serde() {
    let handler = HookHandler::Command {
        command: "lint".to_string(),
        args: vec!["--fix".to_string()],
    };
    let json = serde_json::to_string(&handler).expect("serialize");
    assert!(json.contains("\"type\":\"command\""));

    let parsed: HookHandler = serde_json::from_str(&json).expect("deserialize");
    if let HookHandler::Command { command, args } = parsed {
        assert_eq!(command, "lint");
        assert_eq!(args, vec!["--fix"]);
    } else {
        panic!("Expected Command handler");
    }
}

#[test]
fn test_handler_prompt_serde() {
    let handler = HookHandler::Prompt {
        template: "Review the changes: $ARGUMENTS".to_string(),
        model: None,
    };
    let json = serde_json::to_string(&handler).expect("serialize");
    let parsed: HookHandler = serde_json::from_str(&json).expect("deserialize");
    if let HookHandler::Prompt { template, .. } = parsed {
        assert!(template.contains("$ARGUMENTS"));
    } else {
        panic!("Expected Prompt handler");
    }
}

#[test]
fn test_handler_prompt_with_model() {
    let json = r#"{"type": "prompt", "template": "check $ARGUMENTS", "model": "haiku"}"#;
    let handler: HookHandler = serde_json::from_str(json).expect("deserialize");
    if let HookHandler::Prompt { template, model } = handler {
        assert!(template.contains("$ARGUMENTS"));
        assert_eq!(model, Some("haiku".to_string()));
    } else {
        panic!("Expected Prompt handler");
    }
}

#[test]
fn test_handler_agent_default_turns() {
    let json = r#"{"type": "agent"}"#;
    let handler: HookHandler = serde_json::from_str(json).expect("deserialize");
    if let HookHandler::Agent {
        max_turns,
        prompt,
        timeout,
    } = handler
    {
        assert_eq!(max_turns, 50);
        assert!(prompt.is_none());
        assert_eq!(timeout, 60);
    } else {
        panic!("Expected Agent handler");
    }
}

#[test]
fn test_handler_agent_with_prompt() {
    let json =
        r#"{"type": "agent", "max_turns": 10, "prompt": "verify $ARGUMENTS", "timeout": 120}"#;
    let handler: HookHandler = serde_json::from_str(json).expect("deserialize");
    if let HookHandler::Agent {
        max_turns,
        prompt,
        timeout,
    } = handler
    {
        assert_eq!(max_turns, 10);
        assert_eq!(prompt, Some("verify $ARGUMENTS".to_string()));
        assert_eq!(timeout, 120);
    } else {
        panic!("Expected Agent handler");
    }
}

#[test]
fn test_effective_timeout_clamped() {
    let json = r#"{
        "name": "slow-hook",
        "event_type": "pre_tool_use",
        "handler": { "type": "command", "command": "sleep", "args": [] },
        "timeout_secs": 9999
    }"#;
    let def: HookDefinition = serde_json::from_str(json).expect("parse");
    assert_eq!(def.timeout_secs, 9999);
    assert_eq!(def.effective_timeout_secs(), MAX_TIMEOUT_SECS);
}

#[test]
fn test_effective_timeout_normal() {
    let json = r#"{
        "name": "normal-hook",
        "event_type": "pre_tool_use",
        "handler": { "type": "command", "command": "echo", "args": [] },
        "timeout_secs": 60
    }"#;
    let def: HookDefinition = serde_json::from_str(json).expect("parse");
    assert_eq!(def.effective_timeout_secs(), 60);
}

#[test]
fn test_handler_webhook_serde() {
    let handler = HookHandler::Webhook {
        url: "https://example.com/hook".to_string(),
    };
    let json = serde_json::to_string(&handler).expect("serialize");
    let parsed: HookHandler = serde_json::from_str(&json).expect("deserialize");
    if let HookHandler::Webhook { url } = parsed {
        assert_eq!(url, "https://example.com/hook");
    } else {
        panic!("Expected Webhook handler");
    }
}
