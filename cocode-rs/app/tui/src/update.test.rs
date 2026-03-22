use super::*;
use crate::state::MessageRole;
use cocode_protocol::TokenUsage;

fn create_test_state() -> AppState {
    AppState::new()
}

#[test]
fn test_handle_agent_event_turn_started() {
    let mut state = create_test_state();

    handle_agent_event(
        &mut state,
        LoopEvent::TurnStarted {
            turn_id: "turn-1".to_string(),
            turn_number: 1,
        },
    );

    assert!(state.is_streaming());
    assert_eq!(
        state.ui.streaming.as_ref().map(|s| s.turn_id.as_str()),
        Some("turn-1")
    );
}

#[test]
fn test_handle_agent_event_text_delta() {
    let mut state = create_test_state();
    state.ui.start_streaming("turn-1".to_string());

    handle_agent_event(
        &mut state,
        LoopEvent::TextDelta {
            turn_id: "turn-1".to_string(),
            delta: "Hello ".to_string(),
        },
    );
    handle_agent_event(
        &mut state,
        LoopEvent::TextDelta {
            turn_id: "turn-1".to_string(),
            delta: "World".to_string(),
        },
    );

    assert_eq!(
        state.ui.streaming.as_ref().map(|s| s.content.as_str()),
        Some("Hello World")
    );
}

#[test]
fn test_handle_agent_event_turn_completed() {
    let mut state = create_test_state();
    state.ui.start_streaming("turn-1".to_string());
    state.ui.append_streaming("Test content");

    handle_agent_event(
        &mut state,
        LoopEvent::TurnCompleted {
            turn_id: "turn-1".to_string(),
            usage: TokenUsage::new(100, 50),
        },
    );

    assert!(!state.is_streaming());
    assert_eq!(state.session.messages.len(), 1);
    assert_eq!(state.session.messages[0].content, "Test content");
    assert_eq!(state.session.messages[0].role, MessageRole::Assistant);
}

#[test]
fn test_handle_agent_event_tool_lifecycle() {
    let mut state = create_test_state();

    handle_agent_event(
        &mut state,
        LoopEvent::ToolUseStarted {
            call_id: "call-1".to_string(),
            name: "bash".to_string(),
        },
    );

    assert_eq!(state.session.tool_executions.len(), 1);
    assert_eq!(state.session.tool_executions[0].name, "bash");

    handle_agent_event(
        &mut state,
        LoopEvent::ToolUseCompleted {
            call_id: "call-1".to_string(),
            output: ToolResultContent::Text("Success".to_string()),
            is_error: false,
        },
    );

    assert_eq!(
        state.session.tool_executions[0].output,
        Some("Success".to_string())
    );
}

#[test]
fn test_handle_history_up_down() {
    use crate::state::HistoryEntry;

    let mut state = create_test_state();
    // Add history entries - they're sorted by frecency (most recent first)
    state.ui.input.history = vec![
        HistoryEntry::new("second"), // Index 0 - most recent
        HistoryEntry::new("first"),  // Index 1 - older
    ];

    // Navigate up (goes to older entries, index increases)
    handle_history_up(&mut state);
    assert_eq!(state.ui.input.text(), "second");
    assert_eq!(state.ui.input.history_index, Some(0));

    handle_history_up(&mut state);
    assert_eq!(state.ui.input.text(), "first");
    assert_eq!(state.ui.input.history_index, Some(1));

    // Navigate down (goes to newer entries, index decreases)
    handle_history_down(&mut state);
    assert_eq!(state.ui.input.text(), "second");
    assert_eq!(state.ui.input.history_index, Some(0));

    handle_history_down(&mut state);
    assert!(state.ui.input.is_empty());
    assert_eq!(state.ui.input.history_index, None);
}
