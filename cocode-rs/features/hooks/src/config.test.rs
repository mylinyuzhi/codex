use super::*;

#[test]
fn test_load_hooks_from_toml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("hooks.toml");
    std::fs::write(
        &file,
        r#"
[[hooks]]
name = "lint-check"
event = "pre_tool_use"
timeout_secs = 10

[hooks.matcher]
type = "exact"
value = "bash"

[hooks.handler]
type = "command"
command = "lint"
args = ["--check"]

[[hooks]]
name = "notify-session"
event = "session_start"

[hooks.handler]
type = "prompt"
template = "Session started: $ARGUMENTS"
"#,
    )
    .expect("write");

    let hooks = load_hooks_from_toml(&file).expect("load");
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
    let file = dir.path().join("empty.toml");
    std::fs::write(&file, "").expect("write");

    let hooks = load_hooks_from_toml(&file).expect("load");
    assert!(hooks.is_empty());
}

#[test]
fn test_load_nonexistent_file() {
    let result = load_hooks_from_toml(Path::new("/nonexistent/hooks.toml"));
    assert!(result.is_err());
}

#[test]
fn test_load_invalid_toml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("bad.toml");
    std::fs::write(&file, "this is not valid toml {{{").expect("write");

    let result = load_hooks_from_toml(&file);
    assert!(result.is_err());
}

#[test]
fn test_load_invalid_regex_matcher() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("bad_regex.toml");
    std::fs::write(
        &file,
        r#"
[[hooks]]
name = "bad-regex"
event = "pre_tool_use"

[hooks.matcher]
type = "regex"
pattern = "[invalid"

[hooks.handler]
type = "command"
command = "echo"
"#,
    )
    .expect("write");

    let result = load_hooks_from_toml(&file);
    assert!(result.is_err());
    assert!(result.expect_err("error").contains("invalid matcher"));
}

#[test]
fn test_load_all_handler_types() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("all.toml");
    std::fs::write(
        &file,
        r#"
[[hooks]]
name = "cmd"
event = "pre_tool_use"
[hooks.handler]
type = "command"
command = "echo"

[[hooks]]
name = "prompt"
event = "session_start"
[hooks.handler]
type = "prompt"
template = "hello"

[[hooks]]
name = "agent"
event = "stop"
[hooks.handler]
type = "agent"

[[hooks]]
name = "webhook"
event = "session_end"
[hooks.handler]
type = "webhook"
url = "https://example.com"
"#,
    )
    .expect("write");

    let hooks = load_hooks_from_toml(&file).expect("load");
    assert_eq!(hooks.len(), 4);
    assert!(matches!(hooks[0].handler, HookHandler::Command { .. }));
    assert!(matches!(hooks[1].handler, HookHandler::Prompt { .. }));
    assert!(matches!(hooks[2].handler, HookHandler::Agent { .. }));
    assert!(matches!(hooks[3].handler, HookHandler::Webhook { .. }));
}

#[test]
fn test_load_or_matcher() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("or.toml");
    std::fs::write(
        &file,
        r#"
[[hooks]]
name = "multi-tool"
event = "pre_tool_use"

[hooks.matcher]
type = "or"

[[hooks.matcher.matchers]]
type = "exact"
value = "bash"

[[hooks.matcher.matchers]]
type = "wildcard"
pattern = "read_*"

[hooks.handler]
type = "command"
command = "check"
"#,
    )
    .expect("write");

    let hooks = load_hooks_from_toml(&file).expect("load");
    assert_eq!(hooks.len(), 1);
    let matcher = hooks[0].matcher.as_ref().expect("matcher");
    assert!(matcher.matches("bash"));
    assert!(matcher.matches("read_file"));
    assert!(!matcher.matches("write_file"));
}
