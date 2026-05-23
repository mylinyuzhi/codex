//! Snapshot tests for TUI widgets using insta + native surface test rendering.

use crate::i18n::locale_test_guard;
use crate::state::AppState;
use crate::state::PermissionDetail;
use crate::state::PermissionPromptState;
use crate::state::StreamingState;
use crate::state::SuggestionKind;
use crate::state::Toast;
use crate::state::derive::test_helpers;

fn mark_retained_surface_visible(state: &mut AppState) {
    state
        .ui
        .record_surface_interaction(std::time::Instant::now());
}

fn render_to_string(state: &AppState, width: u16, height: u16) -> String {
    let _locale = locale_test_guard("en");
    crate::testing::render_native_surface_to_string(state, width, height)
}

#[test]
fn test_snapshot_empty_state() {
    let state = AppState::new();
    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("empty_state", output);
}

#[test]
fn test_snapshot_status_bar_full() {
    use crate::state::session::TokenUsage;
    let mut state = AppState::new();
    state.session.model = "deepseek-v4-flash".to_string();
    state.session.provider = "deepseek".to_string();
    state.session.token_usage = TokenUsage {
        input_tokens: 12_300,
        output_tokens: 16_500,
        reasoning_tokens: 0,
        cache_read_tokens: 10_700,
        cache_creation_tokens: 0,
    };
    state.session.context_window_used = 28_000;
    state.session.context_window_total = 200_000;
    let output = render_to_string(&state, 100, 24);
    insta::assert_snapshot!("status_bar_full", output);
}

#[test]
fn test_snapshot_with_messages() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    test_helpers::push_user_text(&mut state.session, "1", "Hello, how are you?");
    test_helpers::push_assistant_text(
        &mut state.session,
        "I'm doing well! How can I help you today?",
    );

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("with_messages", output);
}

#[test]
fn test_snapshot_thinking_collapsed() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    test_helpers::push_user_text(&mut state.session, "1", "hello");
    test_helpers::push_assistant_thinking(
        &mut state.session,
        "The user just said hello, so respond briefly.",
        1600,
        22,
    );
    test_helpers::push_assistant_text(&mut state.session, "Hello! How can I help?");

    let output = render_to_string(&state, 96, 18);
    insta::assert_snapshot!("thinking_collapsed", output);
}

#[test]
fn test_snapshot_thinking_expanded_show_thinking_on() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    state.ui.show_thinking = true;
    test_helpers::push_user_text(&mut state.session, "1", "bash ls");
    test_helpers::push_assistant_thinking(
        &mut state.session,
        "The user wants me to run `ls` in the current working directory.\n\
         I should call the Bash tool and then summarize the result.",
        1300,
        15,
    );
    test_helpers::push_tool_use(&mut state.session, "call-1", "Bash", "ls");
    test_helpers::push_tool_result(
        &mut state.session,
        "call-1",
        "Bash",
        "app\nbridge\ncommon\nutils\nvercel-ai",
        false,
    );

    let output = render_to_string(&state, 96, 22);
    insta::assert_snapshot!("thinking_expanded_show_thinking_on", output);
}

#[test]
fn test_snapshot_with_tool_result() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    test_helpers::push_user_text(&mut state.session, "1", "List files");
    test_helpers::push_tool_use(&mut state.session, "c1", "Bash", "ls -la");
    test_helpers::push_tool_result(
        &mut state.session,
        "c1",
        "Bash",
        "file1.rs\nfile2.rs\nCargo.toml",
        false,
    );

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("with_tool_result", output);
}

#[test]
fn test_snapshot_tool_result_middle_truncation() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    test_helpers::push_user_text(&mut state.session, "1", "find the CLAUDE.md");
    test_helpers::push_tool_use(&mut state.session, "c1", "Glob", "**/CLAUDE.md");
    test_helpers::push_tool_result(
        &mut state.session,
        "c1",
        "Glob",
        "Found 8 files\n\
         coco-rs/app/tui/CLAUDE.md\n\
         coco-rs/app/query/CLAUDE.md\n\
         coco-rs/core/tools/CLAUDE.md\n\
         coco-rs/core/messages/CLAUDE.md\n\
         coco-rs/common/config/CLAUDE.md\n\
         coco-rs/services/inference/CLAUDE.md\n\
         coco-rs/utils/string/CLAUDE.md\n\
         codex-rs/tui/CLAUDE.md",
        false,
    );

    let output = render_to_string(&state, 96, 24);
    insta::assert_snapshot!("tool_result_middle_truncation", output);
}

#[test]
fn assistant_text_after_tool_result_keeps_dot_and_full_body() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    test_helpers::push_user_text(&mut state.session, "1", "find the readme.md");
    test_helpers::push_tool_use(&mut state.session, "c1", "Glob", "**/README.md");
    test_helpers::push_tool_result(
        &mut state.session,
        "c1",
        "Glob",
        "common/error/README.md\n\
         common/otel/README.md\n\
         core/system-reminder/README.md\n\
         vercel-ai/README.md\n\
         app/tui/README.md\n\
         app/query/README.md\n\
         services/inference/README.md",
        false,
    );
    test_helpers::push_assistant_text(
        &mut state.session,
        "Found 13 `README.md` files in the workspace:\n\n\
         | Path | Description |\n\
         |------|-------------|\n\
         | `common/error/README.md` | Error status codes |\n\
         | `common/otel/README.md` | Tracing conventions |\n\
         | `vercel-ai/README.md` | Provider notes |\n\
         | `app/tui/README.md` | TUI notes |",
    );

    let output = render_to_string(&state, 96, 40);
    assert!(output.contains("⏺ Found 13"));
    assert!(output.contains("common/error/README.md"));
    assert!(output.contains("Error status codes"));
    assert!(!output.contains("… output truncated in UI"));
}

#[test]
fn test_snapshot_with_permission_prompt() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    state
        .ui
        .push_prompt(crate::state::PanePromptState::Permission(
            PermissionPromptState {
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
                display_input: coco_types::PermissionDisplayInput::Command(
                    "rm -rf /tmp/test".into(),
                ),
                original_input: None,
                permission_suggestions: vec![],
            },
        ));
    mark_retained_surface_visible(&mut state);

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("permission_prompt", output);
}

#[test]
fn test_snapshot_with_streaming() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    test_helpers::push_user_text(&mut state.session, "1", "Explain Rust");

    let mut streaming = StreamingState::new();
    streaming.append_text("Rust is a systems programming language");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("streaming", output);
}

#[test]
fn test_snapshot_with_help_modal() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    state.ui.show_modal(crate::state::ModalState::Help);

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
        insta::assert_snapshot!("help_modal", output);
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
    // Status bar lives in the row right under the input — content-flow
    // layout matching TS/codex-rs, not at the terminal's last row.
    let status_line = output
        .lines()
        .find(|line| line.contains("Press Ctrl-C again to exit"))
        .unwrap_or_else(|| panic!("exit prompt should render in the status row:\n{output}"));

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
fn test_snapshot_with_error_modal() {
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    let body = crate::widgets::error_dialog::format_error_body(
        "Connection refused after 3 retries",
        Some("network"),
        false,
    );
    state.ui.show_modal(crate::state::ModalState::Error(body));
    mark_retained_surface_visible(&mut state);

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("error_modal_non_retryable", output);
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
    test_helpers::push_user_text(&mut state.session, "1", "Plan the implementation");

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("plan_mode", output);
}

// Edit tool results land as `Message::ToolResult` cells and render
// through the generic tool-result preview — covered by
// `test_snapshot_with_tool_result`.

#[test]
fn test_snapshot_parallel_tool_batch() {
    // Spec: crate-coco-tui.md §ChatWidget Internals — parallel tool_use
    // messages are rendered under a `‖ N in parallel` header.
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    test_helpers::push_user_text(&mut state.session, "1", "Investigate");
    for (i, (name, input)) in [
        ("Bash", "ls -la"),
        ("Read", "src/main.rs"),
        ("Grep", "TODO"),
    ]
    .iter()
    .enumerate()
    {
        test_helpers::push_tool_use(&mut state.session, &format!("c{i}"), name, input);
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
            kind: crate::state::session::SubagentKind::Subagent,
            agent_id: "agent-7af2".into(),
            agent_type: "Explore".into(),
            description: "Search for auth handlers".into(),
            status: SubagentStatus::Running,
            color: Some("blue".into()),
            team_name: None,
            tool_use_id: None,
            started_at_ms: None,
            last_tool_name: None,
            tool_count: 0,
            total_tokens: 0,
            is_backgrounded: false,
            final_message: None,
        },
        SubagentInstance {
            kind: crate::state::session::SubagentKind::Subagent,
            agent_id: "agent-c1d3".into(),
            agent_type: "Plan".into(),
            description: "Design refactor plan".into(),
            status: SubagentStatus::Completed,
            color: None,
            team_name: None,
            tool_use_id: None,
            started_at_ms: None,
            last_tool_name: None,
            tool_count: 0,
            total_tokens: 0,
            is_backgrounded: false,
            final_message: None,
        },
        SubagentInstance {
            kind: crate::state::session::SubagentKind::Subagent,
            agent_id: "agent-9e5f".into(),
            agent_type: "verification".into(),
            description: "Run tests + verify".into(),
            status: SubagentStatus::Failed,
            color: Some("red".into()),
            team_name: None,
            tool_use_id: None,
            started_at_ms: None,
            last_tool_name: None,
            tool_count: 0,
            total_tokens: 0,
            is_backgrounded: false,
            final_message: None,
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
        crate::state::SlashCommandInfo {
            name: "help".into(),
            description: Some("Show help".into()),
            aliases: Vec::new(),
            argument_hint: None,
        },
        crate::state::SlashCommandInfo {
            name: "clear".into(),
            description: Some("Clear chat".into()),
            aliases: Vec::new(),
            argument_hint: None,
        },
        crate::state::SlashCommandInfo {
            name: "config".into(),
            description: Some("Settings".into()),
            aliases: Vec::new(),
            argument_hint: None,
        },
    ];
    state.ui.input.textarea.set_text("/c");
    state.ui.input.textarea.set_cursor(2);
    crate::autocomplete::refresh_suggestions(&mut state);

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("autocomplete_popup", output);
}

#[test]
fn test_snapshot_command_palette_inline_popup() {
    // Spec: TS `PromptInputFooterSuggestions` — Ctrl+P opens the command
    // palette as a borderless list floated above the input. Typed chars
    // route to `cp.filter` and the input bar mirrors `/<filter>` so the
    // user can see what they typed.
    let mut state = AppState::new();
    state.session.model = "opus-4".to_string();
    state.ui.input.textarea.set_text("/c");
    state.ui.input.textarea.set_cursor(2);
    state.ui.active_suggestions = Some(crate::state::ActiveSuggestions {
        kind: SuggestionKind::SlashCommand,
        items: vec![
            crate::widgets::suggestion_popup::SuggestionItem {
                label: "/clear".into(),
                description: Some("Clear conversation".into()),
                metadata: None,
            },
            crate::widgets::suggestion_popup::SuggestionItem {
                label: "/compact".into(),
                description: Some("Compact conversation".into()),
                metadata: None,
            },
        ],
        selected: 0,
        query: "c".into(),
        trigger_pos: 0,
    });
    state.ui.sync_popup_from_active_suggestions();

    let output = render_to_string(&state, 80, 24);
    insta::assert_snapshot!("command_palette_inline", output);
}
