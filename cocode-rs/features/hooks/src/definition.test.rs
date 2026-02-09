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
    };
    let json = serde_json::to_string(&handler).expect("serialize");
    let parsed: HookHandler = serde_json::from_str(&json).expect("deserialize");
    if let HookHandler::Prompt { template } = parsed {
        assert!(template.contains("$ARGUMENTS"));
    } else {
        panic!("Expected Prompt handler");
    }
}

#[test]
fn test_handler_agent_default_turns() {
    let json = r#"{"type": "agent"}"#;
    let handler: HookHandler = serde_json::from_str(json).expect("deserialize");
    if let HookHandler::Agent { max_turns } = handler {
        assert_eq!(max_turns, 5);
    } else {
        panic!("Expected Agent handler");
    }
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
