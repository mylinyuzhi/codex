use super::*;

#[test]
fn test_new() {
    let ctx = HookContext::new(
        HookEventType::PreToolUse,
        "sess-1".to_string(),
        PathBuf::from("/tmp"),
    );
    assert_eq!(ctx.event_type, HookEventType::PreToolUse);
    assert_eq!(ctx.session_id, "sess-1");
    assert_eq!(ctx.working_dir, PathBuf::from("/tmp"));
    assert!(ctx.tool_name.is_none());
    assert!(ctx.tool_input.is_none());
}

#[test]
fn test_builder_pattern() {
    let ctx = HookContext::new(
        HookEventType::PostToolUse,
        "sess-2".to_string(),
        PathBuf::from("/home"),
    )
    .with_tool("read_file", serde_json::json!({"path": "/etc/hosts"}));

    assert_eq!(ctx.tool_name.as_deref(), Some("read_file"));
    assert!(ctx.tool_input.is_some());
}

#[test]
fn test_with_session_id() {
    let ctx = HookContext::new(
        HookEventType::SessionStart,
        "old".to_string(),
        PathBuf::from("/tmp"),
    )
    .with_session_id("new-session");

    assert_eq!(ctx.session_id, "new-session");
}

#[test]
fn test_serde_roundtrip() {
    let ctx = HookContext::new(
        HookEventType::PreToolUse,
        "sess-1".to_string(),
        PathBuf::from("/tmp"),
    )
    .with_tool_name("bash");

    let json = serde_json::to_string(&ctx).expect("serialize");
    let parsed: HookContext = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.event_type, ctx.event_type);
    assert_eq!(parsed.tool_name, ctx.tool_name);
    assert_eq!(parsed.session_id, ctx.session_id);
}

#[test]
fn test_event_specific_fields() {
    // UserPromptSubmit with prompt
    let ctx = HookContext::new(
        HookEventType::UserPromptSubmit,
        "sess".to_string(),
        PathBuf::from("/tmp"),
    )
    .with_prompt("fix the bug");
    assert_eq!(ctx.prompt.as_deref(), Some("fix the bug"));

    // SessionStart with source
    let ctx = HookContext::new(
        HookEventType::SessionStart,
        "sess".to_string(),
        PathBuf::from("/tmp"),
    )
    .with_source("startup");
    assert_eq!(ctx.source.as_deref(), Some("startup"));

    // Stop with stop_hook_active and transcript
    let ctx = HookContext::new(
        HookEventType::Stop,
        "sess".to_string(),
        PathBuf::from("/tmp"),
    )
    .with_stop_hook_active(true)
    .with_transcript(serde_json::json!(["turn1", "turn2"]));
    assert_eq!(ctx.stop_hook_active, Some(true));
    assert!(ctx.transcript.is_some());

    // Notification with notification_type
    let ctx = HookContext::new(
        HookEventType::Notification,
        "sess".to_string(),
        PathBuf::from("/tmp"),
    )
    .with_notification_type("info");
    assert_eq!(ctx.notification_type.as_deref(), Some("info"));
}

#[test]
fn test_match_target() {
    // Tool events match tool_name
    let ctx = HookContext::new(
        HookEventType::PreToolUse,
        "sess".to_string(),
        PathBuf::from("/tmp"),
    )
    .with_tool_name("bash");
    assert_eq!(ctx.match_target(), Some("bash"));

    // SessionStart matches source
    let ctx = HookContext::new(
        HookEventType::SessionStart,
        "sess".to_string(),
        PathBuf::from("/tmp"),
    )
    .with_source("resume");
    assert_eq!(ctx.match_target(), Some("resume"));

    // Notification matches notification_type
    let ctx = HookContext::new(
        HookEventType::Notification,
        "sess".to_string(),
        PathBuf::from("/tmp"),
    )
    .with_notification_type("warning");
    assert_eq!(ctx.match_target(), Some("warning"));

    // No match target when field is not set
    let ctx = HookContext::new(
        HookEventType::SessionStart,
        "sess".to_string(),
        PathBuf::from("/tmp"),
    );
    assert_eq!(ctx.match_target(), None);
}

#[test]
fn test_event_specific_fields_serde_roundtrip() {
    let ctx = HookContext::new(
        HookEventType::Stop,
        "sess".to_string(),
        PathBuf::from("/tmp"),
    )
    .with_stop_hook_active(true)
    .with_transcript(serde_json::json!({"messages": []}));

    let json = serde_json::to_string(&ctx).expect("serialize");
    let parsed: HookContext = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.stop_hook_active, Some(true));
    assert!(parsed.transcript.is_some());
}
