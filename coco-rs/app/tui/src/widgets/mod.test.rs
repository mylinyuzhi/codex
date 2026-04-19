//! Snapshot tests for TUI widgets using insta + ratatui TestBackend.

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::render;
use crate::state::AppState;
use crate::state::Overlay;
use crate::state::PermissionDetail;
use crate::state::PermissionOverlay;
use crate::state::StreamingState;
use crate::state::Toast;
use crate::state::session::ChatMessage;
use crate::state::session::MessageContent;
use crate::state::session::ToolUseStatus;

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
        risk_level: Some(crate::state::RiskLevel::High),
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
fn test_snapshot_with_rate_limit_banner() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    // Blocking rate-limit: remaining=0 keeps the banner visible.
    state.session.rate_limit_info = Some(crate::state::session::RateLimitInfo {
        remaining: Some(0),
        reset_at: None,
        provider: Some("anthropic".to_string()),
    });

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("rate_limit_banner", output);
}

#[test]
fn test_snapshot_with_model_fallback_banner() {
    let mut state = AppState::new();
    state.session.model = "sonnet-4-6".to_string();
    state.session.model_fallback_banner = Some("opus-4-7 → sonnet-4-6".to_string());

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("model_fallback_banner", output);
}

#[test]
fn test_snapshot_with_error_overlay() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    let body = crate::widgets::error_dialog::format_error_body(
        "Connection refused after 3 retries",
        Some("network"),
        false,
    );
    state.ui.set_overlay(Overlay::Error(body));

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("error_overlay_non_retryable", output);
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
    state.session.permission_mode = coco_types::PermissionMode::Plan;
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

#[test]
fn test_snapshot_parallel_tool_batch() {
    // Spec: crate-coco-tui.md §ChatWidget Internals — parallel tool_use
    // messages are rendered under a `‖ N in parallel` header.
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    state
        .session
        .add_message(ChatMessage::user_text("1", "Investigate"));
    for (i, (name, input)) in [
        ("Bash", "ls -la"),
        ("Read", "src/main.rs"),
        ("Grep", "TODO"),
    ]
    .iter()
    .enumerate()
    {
        state.session.add_message(ChatMessage {
            id: format!("m{}", i + 2),
            role: crate::state::ChatRole::Assistant,
            content: MessageContent::ToolUse {
                tool_name: name.to_string(),
                call_id: format!("c{i}"),
                input_preview: input.to_string(),
                status: ToolUseStatus::Running,
            },
            is_meta: false,
            permission_mode: None,
        });
    }

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("parallel_tool_batch", output);
}

#[test]
fn test_snapshot_autocomplete_popup() {
    // Spec: crate-coco-tui.md §Autocomplete Systems — slash command
    // trigger populates the popup synchronously from session state.
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    state.session.available_commands = vec![
        ("help".to_string(), Some("Show help".to_string())),
        ("clear".to_string(), Some("Clear chat".to_string())),
        ("config".to_string(), Some("Settings".to_string())),
    ];
    state.ui.input.text = "/c".to_string();
    state.ui.input.cursor = 2;
    crate::autocomplete::refresh_suggestions(&mut state);

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("autocomplete_popup", output);
}
