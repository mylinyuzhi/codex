use super::*;

#[test]
fn test_session_state_default() {
    let state = SessionState::default();
    assert!(state.messages.is_empty());
    assert!(!state.plan_mode);
    assert!(state.tool_executions.is_empty());
}

#[test]
fn test_add_message() {
    let mut state = SessionState::default();
    state.add_message(ChatMessage::user("1", "Hello"));
    assert_eq!(state.messages.len(), 1);
    assert_eq!(
        state.last_message().map(|m| m.content.as_str()),
        Some("Hello")
    );
}

#[test]
fn test_chat_message_streaming() {
    let mut msg = ChatMessage::streaming_assistant("1");
    assert!(msg.streaming);
    assert!(msg.content.is_empty());

    msg.append("Hello ");
    msg.append("World");
    assert_eq!(msg.content, "Hello World");

    msg.complete();
    assert!(!msg.streaming);
}

#[test]
fn test_tool_lifecycle() {
    let mut state = SessionState::default();

    state.start_tool("call-1".to_string(), "bash".to_string());
    assert_eq!(state.tool_executions.len(), 1);
    assert_eq!(state.tool_executions[0].status, ToolStatus::Running);

    state.update_tool_progress("call-1", "Running...".to_string());
    assert_eq!(
        state.tool_executions[0].progress,
        Some("Running...".to_string())
    );

    state.complete_tool("call-1", "Success".to_string(), false);
    assert_eq!(state.tool_executions[0].status, ToolStatus::Completed);
    assert_eq!(state.tool_executions[0].output, Some("Success".to_string()));
}

#[test]
fn test_cleanup_completed_tools() {
    let mut state = SessionState::default();

    // Add 5 completed tools
    for i in 0..5 {
        state.tool_executions.push(ToolExecution {
            call_id: format!("call-{i}"),
            name: "test".to_string(),
            status: ToolStatus::Completed,
            progress: None,
            output: None,
        });
    }

    // Keep only 2
    state.cleanup_completed_tools(2);
    assert_eq!(state.tool_executions.len(), 2);
}

#[test]
fn test_thinking_budget() {
    let mut state = SessionState::default();

    // Initially no budget
    assert!(state.thinking_budget_tokens.is_none());
    assert_eq!(state.thinking_tokens_used, 0);
    assert!(state.thinking_budget_remaining().is_none());

    // Set a budget
    state.set_thinking_budget(Some(10000));
    assert_eq!(state.thinking_budget_remaining(), Some(10000));

    // Add tokens used
    state.add_thinking_tokens(3000);
    assert_eq!(state.thinking_tokens_used, 3000);
    assert_eq!(state.thinking_budget_remaining(), Some(7000));

    // Add more tokens
    state.add_thinking_tokens(5000);
    assert_eq!(state.thinking_tokens_used, 8000);
    assert_eq!(state.thinking_budget_remaining(), Some(2000));

    // Reset tokens
    state.reset_thinking_tokens();
    assert_eq!(state.thinking_tokens_used, 0);
    assert_eq!(state.thinking_budget_remaining(), Some(10000));

    // Over budget should return 0
    state.add_thinking_tokens(15000);
    assert_eq!(state.thinking_budget_remaining(), Some(0));
}
