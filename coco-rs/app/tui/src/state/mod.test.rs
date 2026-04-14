//! Tests for TUI AppState.

use crate::state::AppState;
use crate::state::session::ChatMessage;
use crate::state::session::ChatRole;
use crate::state::session::TokenUsage;
use crate::state::ui::Overlay;
use crate::state::ui::PermissionDetail;
use crate::state::ui::PermissionOverlay;
use crate::state::ui::Toast;

#[test]
fn test_new_state_defaults() {
    let state = AppState::new();
    assert!(!state.should_exit());
    assert!(!state.has_overlay());
    assert!(!state.is_streaming());
    assert!(!state.should_show_spinner());
    assert!(!state.session.plan_mode);
    assert!(!state.session.fast_mode);
    assert_eq!(state.session.turn_count, 0);
    assert_eq!(state.session.messages.len(), 0);
}

#[test]
fn test_overlay_queue() {
    let mut state = AppState::new();

    // First overlay becomes active
    state.ui.set_overlay(Overlay::Help);
    assert!(state.has_overlay());

    // Second overlay is queued
    state.ui.set_overlay(Overlay::Error("test".to_string()));
    assert!(matches!(state.ui.overlay, Some(Overlay::Help)));
    assert_eq!(state.ui.overlay_queue.len(), 1);

    // Dismiss shows next
    state.ui.dismiss_overlay();
    assert!(matches!(state.ui.overlay, Some(Overlay::Error(_))));
    assert_eq!(state.ui.overlay_queue.len(), 0);

    // Dismiss clears
    state.ui.dismiss_overlay();
    assert!(!state.has_overlay());
}

#[test]
fn test_toast_lifecycle() {
    let mut state = AppState::new();

    state.ui.add_toast(Toast::info("hello"));
    assert!(state.ui.has_toasts());
    assert_eq!(state.ui.toasts.len(), 1);

    // Toasts should not be expired immediately
    state.ui.expire_toasts();
    assert!(state.ui.has_toasts());
}

#[test]
fn test_session_messages() {
    let mut state = AppState::new();

    state
        .session
        .add_message(ChatMessage::user_text("1", "hello"));
    state
        .session
        .add_message(ChatMessage::assistant_text("2", "hi there"));

    assert_eq!(state.session.messages.len(), 2);
    assert_eq!(
        state.session.last_message().map(|m| m.role),
        Some(ChatRole::Assistant)
    );
}

#[test]
fn test_tool_execution_lifecycle() {
    let mut state = AppState::new();

    state
        .session
        .start_tool("call-1".to_string(), "Bash".to_string());
    assert_eq!(state.session.tool_executions.len(), 1);
    assert_eq!(
        state.session.tool_executions[0].status,
        crate::state::session::ToolStatus::Running
    );

    state.session.complete_tool("call-1", /*is_error*/ false);
    assert_eq!(
        state.session.tool_executions[0].status,
        crate::state::session::ToolStatus::Completed
    );

    state.session.complete_tool("call-2", /*is_error*/ true);
    // Non-existent tool: no panic
}

#[test]
fn test_input_editing() {
    let mut state = AppState::new();

    state.ui.input.insert_char('h');
    state.ui.input.insert_char('i');
    assert_eq!(state.ui.input.text, "hi");
    assert_eq!(state.ui.input.cursor, 2);

    state.ui.input.cursor_left();
    assert_eq!(state.ui.input.cursor, 1);

    state.ui.input.insert_char('!');
    assert_eq!(state.ui.input.text, "h!i");

    let taken = state.ui.input.take_input();
    assert_eq!(taken, "h!i");
    assert!(state.ui.input.is_empty());
    assert_eq!(state.ui.input.cursor, 0);
}

#[test]
fn test_input_history() {
    let mut state = AppState::new();

    state.ui.input.add_to_history("first".to_string());
    state.ui.input.add_to_history("second".to_string());
    assert_eq!(state.ui.input.history.len(), 2);

    // Duplicate removal
    state.ui.input.add_to_history("first".to_string());
    assert_eq!(state.ui.input.history.len(), 2);
    assert_eq!(state.ui.input.history[1], "first");
}

#[test]
fn test_permission_overlay() {
    let mut state = AppState::new();

    state.ui.set_overlay(Overlay::Permission(PermissionOverlay {
        request_id: "req-1".to_string(),
        tool_name: "Bash".to_string(),
        description: "Run command".to_string(),
        detail: PermissionDetail::Bash {
            command: "ls -la".to_string(),
            risk_description: None,
            working_dir: None,
        },
        risk_level: None,
        show_always_allow: true,
        classifier_checking: false,
        classifier_auto_approved: None,
    }));

    assert!(state.has_overlay());
    assert!(matches!(state.ui.overlay, Some(Overlay::Permission(_))));
}

#[test]
fn test_streaming_state() {
    let mut state = AppState::new();
    assert!(!state.is_streaming());

    state.ui.streaming = Some(crate::state::ui::StreamingState::new());
    assert!(state.is_streaming());
    assert!(state.should_show_spinner());

    if let Some(ref mut s) = state.ui.streaming {
        s.append_text("hello ");
        s.append_text("world\n");
        assert_eq!(s.content, "hello world\n");
        assert_eq!(s.visible_content(), ""); // cursor at 0

        s.advance_display();
        assert_eq!(s.visible_content(), "hello world\n");
    }
}

#[test]
fn test_token_usage_update() {
    let mut state = AppState::new();

    state.session.update_tokens(TokenUsage {
        input_tokens: 100,
        output_tokens: 50,
        cache_read_tokens: 20,
        cache_creation_tokens: 10,
    });

    assert_eq!(state.session.token_usage.input_tokens, 100);
    assert_eq!(state.session.token_usage.output_tokens, 50);
}
