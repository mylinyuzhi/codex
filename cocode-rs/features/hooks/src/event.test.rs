use super::*;

#[test]
fn test_as_str() {
    assert_eq!(HookEventType::PreToolUse.as_str(), "pre_tool_use");
    assert_eq!(HookEventType::PostToolUse.as_str(), "post_tool_use");
    assert_eq!(
        HookEventType::PostToolUseFailure.as_str(),
        "post_tool_use_failure"
    );
    assert_eq!(
        HookEventType::UserPromptSubmit.as_str(),
        "user_prompt_submit"
    );
    assert_eq!(HookEventType::SessionStart.as_str(), "session_start");
    assert_eq!(HookEventType::SessionEnd.as_str(), "session_end");
    assert_eq!(HookEventType::Stop.as_str(), "stop");
    assert_eq!(HookEventType::SubagentStart.as_str(), "subagent_start");
    assert_eq!(HookEventType::SubagentStop.as_str(), "subagent_stop");
    assert_eq!(HookEventType::PreCompact.as_str(), "pre_compact");
    assert_eq!(HookEventType::Notification.as_str(), "notification");
    assert_eq!(
        HookEventType::PermissionRequest.as_str(),
        "permission_request"
    );
    assert_eq!(HookEventType::TeammateIdle.as_str(), "teammate_idle");
    assert_eq!(HookEventType::TaskCompleted.as_str(), "task_completed");
}

#[test]
fn test_display() {
    assert_eq!(format!("{}", HookEventType::PreToolUse), "pre_tool_use");
    assert_eq!(format!("{}", HookEventType::Stop), "stop");
}

#[test]
fn test_serde_roundtrip() {
    let event = HookEventType::PostToolUse;
    let json = serde_json::to_string(&event).expect("serialize");
    assert_eq!(json, "\"post_tool_use\"");
    let parsed: HookEventType = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, event);
}

#[test]
fn test_clone_eq_hash() {
    let a = HookEventType::SessionStart;
    let b = a.clone();
    assert_eq!(a, b);

    // Test Hash by inserting into a HashSet
    let mut set = std::collections::HashSet::new();
    set.insert(a);
    assert!(set.contains(&b));
}
