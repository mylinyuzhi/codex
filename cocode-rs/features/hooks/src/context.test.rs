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
