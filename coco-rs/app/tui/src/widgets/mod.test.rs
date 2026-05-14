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
        is_compact_summary: false,
        is_visible_in_transcript_only: false,
        created_at_ms: 0,
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
        choices: None,
        selected_choice: 0,
        original_input: None,
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
    // Modifier label depends on host OS (`opt` on macOS, `alt`
    // elsewhere) — see `coco_keybindings::display::DisplayPlatform`.
    // Per-platform snapshot suffix keeps both labels honest instead
    // of normalizing one away.
    let suffix = if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    insta::with_settings!({ snapshot_suffix => suffix }, {
        insta::assert_snapshot!("help_overlay", output);
    });
}

#[test]
fn test_snapshot_with_pending_chord() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyEventKind;
    use crossterm::event::KeyEventState;
    use crossterm::event::KeyModifiers;

    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();

    // Feed `ctrl+x` (a chord prefix from defaults — `ctrl+x ctrl+k`
    // = chat:killAgents, `ctrl+x ctrl+e` = chat:externalEditor) so
    // the per-state resolver enters Pending. The status bar should
    // then render `"ctrl+x …"` between the permission-mode and token
    // segments.
    let event = KeyEvent {
        code: KeyCode::Char('x'),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    };
    let outcome = state
        .ui
        .kb_handle
        .resolve_key(event, crate::keybinding_bridge::KeybindingContext::Chat);
    // Sanity: resolver must really be in Pending — otherwise the
    // status-bar hint won't render and the snapshot would silently
    // drift.
    assert!(
        matches!(outcome, crate::keybinding_resolver::ResolverResult::Pending),
        "expected Pending, got {outcome:?}",
    );

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("pending_chord", output);
}

#[test]
fn test_exit_prompt_replaces_status_bar() {
    let mut state = AppState::new();
    state.ui.ctrl_c_tracker.poll((), std::time::Instant::now());

    let output = render_to_string(&state, 80, 24);
    let status_line = output
        .lines()
        .nth(23)
        .expect("24-line render should include a status row");

    assert!(
        status_line.contains("Press Ctrl-C again to exit"),
        "exit prompt should render in the bottom status bar:\n{output}",
    );
    assert!(
        !status_line.contains("Default") && !status_line.contains("msgs"),
        "exit prompt should replace model/mode/message-count status content:\n{output}",
    );
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
        is_compact_summary: false,
        is_visible_in_transcript_only: false,
        created_at_ms: 0,
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
            is_compact_summary: false,
            is_visible_in_transcript_only: false,
            created_at_ms: 0,
            permission_mode: None,
        });
    }

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("parallel_tool_batch", output);
}

#[test]
fn test_snapshot_subagent_panel_populated() {
    // P2 / A5: subagent panel must render mixed status states
    // (Running/Completed/Failed/Backgrounded) without truncating
    // the description or losing the focus indicator.
    use crate::state::session::SubagentInstance;
    use crate::state::session::SubagentStatus;
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    state.session.subagents = vec![
        SubagentInstance {
            agent_id: "agent-7af2".into(),
            agent_type: "Explore".into(),
            description: "Search for auth handlers".into(),
            status: SubagentStatus::Running,
            color: Some("blue".into()),
            started_at_ms: None,
            token_usage: None,
        },
        SubagentInstance {
            agent_id: "agent-c1d3".into(),
            agent_type: "Plan".into(),
            description: "Design refactor plan".into(),
            status: SubagentStatus::Completed,
            color: None,
            started_at_ms: None,
            token_usage: None,
        },
        SubagentInstance {
            agent_id: "agent-9e5f".into(),
            agent_type: "verification".into(),
            description: "Run tests + verify".into(),
            status: SubagentStatus::Failed,
            color: Some("red".into()),
            started_at_ms: None,
            token_usage: None,
        },
    ];
    state.session.focused_subagent_index = Some(0);
    let output = render_to_string(&state, 100, 24);
    insta::assert_snapshot!("subagent_panel_populated", output);
}

#[test]
fn test_snapshot_prompt_suggestion_renders_as_dim_placeholder() {
    // P5 / A4: when input is empty AND a prompt suggestion is
    // available, the suggestion replaces the default placeholder.
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    state.session.prompt_suggestions = vec!["Run the failing tests".into()];
    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("prompt_suggestion_placeholder", output);
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
