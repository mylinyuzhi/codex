use super::*;

fn make_user_message() -> TrackedMessage {
    TrackedMessage::user("Hello", "turn-1")
}

fn make_tool_call() -> TrackedToolCall {
    TrackedToolCall::from_parts("call-1", "get_weather", serde_json::json!({"city": "NYC"}))
}

#[test]
fn test_tool_call_lifecycle() {
    let mut tc = make_tool_call();
    assert!(matches!(tc.status, ToolCallStatus::Pending));
    assert!(tc.completed_at.is_none());

    tc.start();
    assert!(tc.status.is_running());

    tc.complete(ToolResultContent::Text("Sunny, 72°F".to_string()));
    assert!(tc.status.is_success());
    assert!(tc.completed_at.is_some());
    assert!(tc.output.is_some());
}

#[test]
fn test_tool_call_failure() {
    let mut tc = make_tool_call();
    tc.start();
    tc.fail("Network error");

    assert!(matches!(tc.status, ToolCallStatus::Failed { .. }));
    assert!(tc.status.is_terminal());
}

#[test]
fn test_tool_call_abort() {
    let mut tc = make_tool_call();
    tc.start();
    tc.abort(AbortReason::UserInterrupted);

    assert!(matches!(tc.status, ToolCallStatus::Aborted { .. }));
    assert!(tc.status.is_terminal());
}

#[test]
fn test_turn_creation() {
    let user_msg = make_user_message();
    let turn = Turn::new(1, user_msg);

    assert_eq!(turn.number, 1);
    assert!(!turn.is_complete());
    assert!(turn.assistant_message.is_none());
    assert!(turn.tool_calls.is_empty());
}

#[test]
fn test_turn_with_tool_calls() {
    let user_msg = make_user_message();
    let mut turn = Turn::new(1, user_msg);

    turn.add_tool_call(make_tool_call());
    assert_eq!(turn.tool_calls.len(), 1);
    assert_eq!(turn.pending_tool_count(), 1);

    // Complete the tool call
    let tc = turn.get_tool_call_mut("call-1").unwrap();
    tc.complete(ToolResultContent::Text("done".to_string()));

    assert_eq!(turn.pending_tool_count(), 0);
    assert!(turn.all_tools_complete());
}

#[test]
fn test_turn_usage() {
    let user_msg = make_user_message();
    let mut turn = Turn::new(1, user_msg);

    turn.update_usage(TokenUsage::new(100, 50));
    turn.update_usage(TokenUsage::new(50, 25));

    assert_eq!(turn.usage.input_tokens, 150);
    assert_eq!(turn.usage.output_tokens, 75);
}

#[test]
fn test_turn_completion() {
    let user_msg = make_user_message();
    let mut turn = Turn::new(1, user_msg);

    assert!(!turn.is_complete());
    turn.complete();
    assert!(turn.is_complete());
    assert!(turn.duration().is_some());
}
