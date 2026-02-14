use super::*;

#[test]
fn test_load_hooks_from_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("hooks.json");
    std::fs::write(
        &file,
        r#"{
  "hooks": [
    {
      "name": "lint-check",
      "event": "pre_tool_use",
      "timeout_secs": 10,
      "matcher": {
        "type": "exact",
        "value": "bash"
      },
      "handler": {
        "type": "command",
        "command": "lint",
        "args": ["--check"]
      }
    },
    {
      "name": "notify-session",
      "event": "session_start",
      "handler": {
        "type": "prompt",
        "template": "Session started: $ARGUMENTS"
      }
    }
  ]
}"#,
    )
    .expect("write");

    let hooks = load_hooks_from_json(&file).expect("load");
    assert_eq!(hooks.len(), 2);

    assert_eq!(hooks[0].name, "lint-check");
    assert_eq!(hooks[0].event_type, HookEventType::PreToolUse);
    assert_eq!(hooks[0].timeout_secs, 10);
    assert!(hooks[0].matcher.is_some());
    assert!(hooks[0].enabled);

    assert_eq!(hooks[1].name, "notify-session");
    assert_eq!(hooks[1].event_type, HookEventType::SessionStart);
    assert_eq!(hooks[1].timeout_secs, 30); // default
    assert!(hooks[1].matcher.is_none());
}

#[test]
fn test_load_empty_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("empty.json");
    std::fs::write(&file, "{}").expect("write");

    let hooks = load_hooks_from_json(&file).expect("load");
    assert!(hooks.is_empty());
}

#[test]
fn test_load_nonexistent_file() {
    let result = load_hooks_from_json(Path::new("/nonexistent/hooks.json"));
    assert!(result.is_err());
}

#[test]
fn test_load_invalid_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("bad.json");
    std::fs::write(&file, "this is not valid json {{{").expect("write");

    let result = load_hooks_from_json(&file);
    assert!(result.is_err());
}

#[test]
fn test_load_invalid_regex_matcher() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("bad_regex.json");
    std::fs::write(
        &file,
        r#"{
  "hooks": [
    {
      "name": "bad-regex",
      "event": "pre_tool_use",
      "matcher": {
        "type": "regex",
        "pattern": "[invalid"
      },
      "handler": {
        "type": "command",
        "command": "echo"
      }
    }
  ]
}"#,
    )
    .expect("write");

    let result = load_hooks_from_json(&file);
    assert!(result.is_err());
    assert!(result.expect_err("error").contains("invalid matcher"));
}

#[test]
fn test_load_all_handler_types() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("all.json");
    std::fs::write(
        &file,
        r#"{
  "hooks": [
    {
      "name": "cmd",
      "event": "pre_tool_use",
      "handler": { "type": "command", "command": "echo" }
    },
    {
      "name": "prompt",
      "event": "session_start",
      "handler": { "type": "prompt", "template": "hello" }
    },
    {
      "name": "agent",
      "event": "stop",
      "handler": { "type": "agent" }
    },
    {
      "name": "webhook",
      "event": "session_end",
      "handler": { "type": "webhook", "url": "https://example.com" }
    }
  ]
}"#,
    )
    .expect("write");

    let hooks = load_hooks_from_json(&file).expect("load");
    assert_eq!(hooks.len(), 4);
    assert!(matches!(hooks[0].handler, HookHandler::Command { .. }));
    assert!(matches!(hooks[1].handler, HookHandler::Prompt { .. }));
    assert!(matches!(hooks[2].handler, HookHandler::Agent { .. }));
    assert!(matches!(hooks[3].handler, HookHandler::Webhook { .. }));
}

#[test]
fn test_load_or_matcher() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("or.json");
    std::fs::write(
        &file,
        r#"{
  "hooks": [
    {
      "name": "multi-tool",
      "event": "pre_tool_use",
      "matcher": {
        "type": "or",
        "matchers": [
          { "type": "exact", "value": "bash" },
          { "type": "wildcard", "pattern": "read_*" }
        ]
      },
      "handler": {
        "type": "command",
        "command": "check"
      }
    }
  ]
}"#,
    )
    .expect("write");

    let hooks = load_hooks_from_json(&file).expect("load");
    assert_eq!(hooks.len(), 1);
    let matcher = hooks[0].matcher.as_ref().expect("matcher");
    assert!(matcher.matches("bash"));
    assert!(matcher.matches("read_file"));
    assert!(!matcher.matches("write_file"));
}
