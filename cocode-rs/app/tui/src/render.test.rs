use super::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn create_test_terminal() -> Terminal<TestBackend> {
    let backend = TestBackend::new(80, 24);
    Terminal::new(backend).expect("Failed to create test terminal")
}

#[test]
fn test_render_empty_state() {
    let mut terminal = create_test_terminal();
    let state = AppState::new();

    terminal
        .draw(|frame| {
            render(frame, &state);
        })
        .expect("Failed to render");

    // Just verify it doesn't panic
}

#[test]
fn test_render_with_messages() {
    let mut terminal = create_test_terminal();
    let mut state = AppState::new();

    state
        .session
        .add_message(crate::state::ChatMessage::user("1", "Hello"));
    state
        .session
        .add_message(crate::state::ChatMessage::assistant("2", "Hi there!"));

    terminal
        .draw(|frame| {
            render(frame, &state);
        })
        .expect("Failed to render");
}

#[test]
fn test_render_with_streaming() {
    let mut terminal = create_test_terminal();
    let mut state = AppState::new();

    state.ui.start_streaming("turn-1".to_string());
    state.ui.append_streaming("Streaming content...");

    terminal
        .draw(|frame| {
            render(frame, &state);
        })
        .expect("Failed to render");
}

#[test]
fn test_render_with_tools() {
    let mut terminal = create_test_terminal();
    let mut state = AppState::new();

    state
        .session
        .start_tool("call-1".to_string(), "bash".to_string());

    terminal
        .draw(|frame| {
            render(frame, &state);
        })
        .expect("Failed to render");
}

#[test]
fn test_render_with_permission_overlay() {
    let mut terminal = create_test_terminal();
    let mut state = AppState::new();

    state
        .ui
        .set_overlay(Overlay::Permission(crate::state::PermissionOverlay::new(
            cocode_protocol::ApprovalRequest {
                request_id: "req-1".to_string(),
                tool_name: "bash".to_string(),
                description: "Run command: ls -la".to_string(),
                risks: vec![],
                allow_remember: true,
                proposed_prefix_pattern: None,
            },
        )));

    terminal
        .draw(|frame| {
            render(frame, &state);
        })
        .expect("Failed to render");
}

#[test]
fn test_render_with_model_picker() {
    let mut terminal = create_test_terminal();
    let mut state = AppState::new();

    state
        .ui
        .set_overlay(Overlay::ModelPicker(crate::state::ModelPickerOverlay::new(
            vec!["claude-sonnet-4".to_string(), "claude-opus-4".to_string()],
        )));

    terminal
        .draw(|frame| {
            render(frame, &state);
        })
        .expect("Failed to render");
}

#[test]
fn test_render_with_error_overlay() {
    let mut terminal = create_test_terminal();
    let mut state = AppState::new();

    state
        .ui
        .set_overlay(Overlay::Error("Something went wrong".to_string()));

    terminal
        .draw(|frame| {
            render(frame, &state);
        })
        .expect("Failed to render");
}
