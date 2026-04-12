//! Snapshot tests for TUI widgets using insta + ratatui TestBackend.

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::render;
use crate::state::AppState;
use crate::state::session::ChatMessage;
use crate::state::session::MessageContent;
use crate::state::session::ToolUseStatus;
use crate::state::ui::Overlay;
use crate::state::ui::PermissionDetail;
use crate::state::ui::PermissionOverlay;
use crate::state::ui::StreamingState;
use crate::state::ui::Toast;

fn render_to_string(state: &AppState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal creation");
    terminal
        .draw(|frame| render::render(frame, state))
        .expect("render");
    let buf = terminal.backend().buffer().clone();

    let mut output = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            output.push_str(cell.symbol());
        }
        output.push('\n');
    }
    output
}

#[test]
fn test_snapshot_empty_state() {
    let state = AppState::new();
    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("empty_state", output);
}

#[test]
fn test_snapshot_with_messages() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    state
        .session
        .add_message(ChatMessage::user_text("1", "Hello, how are you?"));
    state.session.add_message(ChatMessage::assistant_text(
        "2",
        "I'm doing well! How can I help you today?",
    ));

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("with_messages", output);
}

#[test]
fn test_snapshot_with_tool_result() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    state
        .session
        .add_message(ChatMessage::user_text("1", "List files"));
    state.session.add_message(ChatMessage {
        id: "2".to_string(),
        role: crate::state::ChatRole::Assistant,
        content: MessageContent::ToolUse {
            tool_name: "Bash".to_string(),
            call_id: "c1".to_string(),
            input_preview: "ls -la".to_string(),
            status: ToolUseStatus::Completed,
        },
        is_meta: false,
        permission_mode: None,
    });
    state.session.add_message(ChatMessage::tool_success(
        "3",
        "Bash",
        "file1.rs\nfile2.rs\nCargo.toml",
    ));

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("with_tool_result", output);
}

#[test]
fn test_snapshot_with_permission_overlay() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    state.ui.set_overlay(Overlay::Permission(PermissionOverlay {
        request_id: "r1".to_string(),
        tool_name: "Bash".to_string(),
        description: "Execute shell command".to_string(),
        detail: PermissionDetail::Bash {
            command: "rm -rf /tmp/test".to_string(),
            risk_description: Some("Removes files recursively".to_string()),
            working_dir: Some("/home/user/project".to_string()),
        },
        risk_level: Some(crate::state::ui::RiskLevel::High),
        show_always_allow: true,
        classifier_checking: false,
        classifier_auto_approved: None,
    }));

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("permission_overlay", output);
}

#[test]
fn test_snapshot_with_streaming() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    state
        .session
        .add_message(ChatMessage::user_text("1", "Explain Rust"));

    let mut streaming = StreamingState::new();
    streaming.append_text("Rust is a systems programming language");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("streaming", output);
}

#[test]
fn test_snapshot_with_help_overlay() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    state.ui.set_overlay(Overlay::Help);

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("help_overlay", output);
}

#[test]
fn test_snapshot_with_toast() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    state.ui.add_toast(Toast::success("Operation completed"));
    state.ui.add_toast(Toast::error("Something went wrong"));

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("with_toasts", output);
}

#[test]
fn test_snapshot_plan_mode() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    state.session.plan_mode = true;
    state.session.turn_count = 3;
    state
        .session
        .add_message(ChatMessage::user_text("1", "Plan the implementation"));

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("plan_mode", output);
}

#[test]
fn test_snapshot_file_diff() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    state.session.add_message(ChatMessage {
        id: "1".to_string(),
        role: crate::state::ChatRole::Tool,
        content: MessageContent::FileEditDiff {
            path: "src/main.rs".to_string(),
            diff: "+fn main() {\n+    println!(\"hello\");\n+}\n-fn main() {}".to_string(),
            old_content: None,
            new_content: None,
        },
        is_meta: false,
        permission_mode: None,
    });

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("file_diff", output);
}
