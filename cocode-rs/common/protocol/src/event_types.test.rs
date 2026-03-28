use super::*;

#[test]
fn test_token_usage() {
    let usage = TokenUsage::new(100, 50);
    assert_eq!(usage.input_tokens, 100i64);
    assert_eq!(usage.output_tokens, 50i64);
    assert_eq!(usage.total(), 150i64);
}

#[test]
fn test_abort_reason() {
    assert_eq!(
        AbortReason::StreamingFallback.as_str(),
        "streaming_fallback"
    );
    assert_eq!(AbortReason::SiblingError.as_str(), "sibling_error");
    assert_eq!(AbortReason::UserInterrupted.as_str(), "user_interrupted");
}

#[test]
fn test_hook_event_type() {
    assert_eq!(HookEventType::PreToolUse.as_str(), "pre_tool_use");
    assert_eq!(HookEventType::PostToolUse.as_str(), "post_tool_use");
    assert_eq!(
        HookEventType::PostToolUseFailure.as_str(),
        "post_tool_use_failure"
    );
    assert_eq!(HookEventType::SessionStart.as_str(), "session_start");
    assert_eq!(HookEventType::PreCompact.as_str(), "pre_compact");
    assert_eq!(HookEventType::PostCompact.as_str(), "post_compact");
    assert_eq!(HookEventType::Stop.as_str(), "stop");
    assert_eq!(HookEventType::SubagentStart.as_str(), "subagent_start");
    assert_eq!(HookEventType::SubagentStop.as_str(), "subagent_stop");
}

#[test]
fn test_hook_event_type_from_str_snake_case() {
    assert_eq!(
        "pre_tool_use".parse::<HookEventType>().unwrap(),
        HookEventType::PreToolUse
    );
    assert_eq!(
        "post_tool_use".parse::<HookEventType>().unwrap(),
        HookEventType::PostToolUse
    );
    assert_eq!(
        "post_tool_use_failure".parse::<HookEventType>().unwrap(),
        HookEventType::PostToolUseFailure
    );
    assert_eq!(
        "user_prompt_submit".parse::<HookEventType>().unwrap(),
        HookEventType::UserPromptSubmit
    );
    assert_eq!(
        "session_start".parse::<HookEventType>().unwrap(),
        HookEventType::SessionStart
    );
    assert_eq!(
        "session_end".parse::<HookEventType>().unwrap(),
        HookEventType::SessionEnd
    );
    assert_eq!(
        "stop".parse::<HookEventType>().unwrap(),
        HookEventType::Stop
    );
    assert_eq!(
        "subagent_start".parse::<HookEventType>().unwrap(),
        HookEventType::SubagentStart
    );
    assert_eq!(
        "subagent_stop".parse::<HookEventType>().unwrap(),
        HookEventType::SubagentStop
    );
    assert_eq!(
        "pre_compact".parse::<HookEventType>().unwrap(),
        HookEventType::PreCompact
    );
    assert_eq!(
        "post_compact".parse::<HookEventType>().unwrap(),
        HookEventType::PostCompact
    );
    assert_eq!(
        "notification".parse::<HookEventType>().unwrap(),
        HookEventType::Notification
    );
    assert_eq!(
        "permission_request".parse::<HookEventType>().unwrap(),
        HookEventType::PermissionRequest
    );
    assert_eq!(
        "teammate_idle".parse::<HookEventType>().unwrap(),
        HookEventType::TeammateIdle
    );
    assert_eq!(
        "task_completed".parse::<HookEventType>().unwrap(),
        HookEventType::TaskCompleted
    );
}

#[test]
fn test_hook_event_type_from_str_pascal_case() {
    assert_eq!(
        "PreToolUse".parse::<HookEventType>().unwrap(),
        HookEventType::PreToolUse
    );
    assert_eq!(
        "PostToolUse".parse::<HookEventType>().unwrap(),
        HookEventType::PostToolUse
    );
    assert_eq!(
        "SessionStart".parse::<HookEventType>().unwrap(),
        HookEventType::SessionStart
    );
    assert_eq!(
        "Stop".parse::<HookEventType>().unwrap(),
        HookEventType::Stop
    );
    assert_eq!(
        "TeammateIdle".parse::<HookEventType>().unwrap(),
        HookEventType::TeammateIdle
    );
    assert_eq!(
        "TaskCompleted".parse::<HookEventType>().unwrap(),
        HookEventType::TaskCompleted
    );
}

#[test]
fn test_hook_event_type_from_str_unknown() {
    assert!("unknown_event".parse::<HookEventType>().is_err());
    let err = "bogus".parse::<HookEventType>().unwrap_err();
    assert!(err.contains("unknown hook event type"));
}

#[test]
fn test_retry_info() {
    let info = RetryInfo {
        attempt: 1,
        max_attempts: 3,
        delay_ms: 1000,
        retriable: true,
    };

    let json = serde_json::to_string(&info).unwrap();
    let parsed: RetryInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, info);
}

#[test]
fn test_tool_result_content() {
    let text = ToolResultContent::Text("Hello".to_string());
    let json = serde_json::to_string(&text).unwrap();
    assert_eq!(json, "\"Hello\"");

    let structured = ToolResultContent::Structured(serde_json::json!({"key": "value"}));
    let json = serde_json::to_string(&structured).unwrap();
    assert!(json.contains("key"));
}

#[test]
fn test_mcp_startup_status() {
    let status = McpStartupStatus::Ready;
    let json = serde_json::to_string(&status).unwrap();
    assert_eq!(json, "\"ready\"");

    let parsed: McpStartupStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, McpStartupStatus::Ready);
}

#[test]
fn test_rewind_mode_serde() {
    let modes = vec![
        RewindMode::CodeAndConversation,
        RewindMode::ConversationOnly,
        RewindMode::CodeOnly,
    ];

    for mode in modes {
        let json = serde_json::to_string(&mode).unwrap();
        let parsed: RewindMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, parsed);
    }
}
