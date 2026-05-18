use super::*;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::i18n::locale_test_guard;
use crate::presentation::streaming::StreamingTailBlock;
use crate::presentation::streaming::StreamingTailView;
use crate::presentation::styles::UiStyles;
use crate::state::session::ChatMessage;
use crate::state::session::ChatRole;
use crate::state::session::MessageContent;
use crate::state::session::ToolExecution;
use crate::state::session::ToolStatus;
use crate::state::session::ToolUseStatus;
use crate::state::transcript::TranscriptCellId;
use crate::state::transcript::TranscriptState;
use crate::state::ui::StreamingState;
use crate::theme::Theme;
use crate::widgets::TranscriptStateWidget;

#[test]
fn test_tool_output_preview_empty_output() {
    assert_eq!(tool_output_preview("", 5), ToolOutputPreview::Empty);
}

#[test]
fn test_tool_output_preview_short_output_keeps_all_lines() {
    assert_eq!(
        tool_output_preview("one\ntwo\nthree", 5),
        ToolOutputPreview::Full(vec!["one", "two", "three"])
    );
}

#[test]
fn test_tool_output_preview_long_output_keeps_head_and_tail() {
    assert_eq!(
        tool_output_preview("one\ntwo\nthree\nfour\nfive\nsix\nseven\neight", 5),
        ToolOutputPreview::Truncated {
            head: vec!["one", "two"],
            omitted: 4,
            tail: vec!["seven", "eight"],
        }
    );
}

#[test]
fn test_tool_output_preview_one_row_budget_reports_omitted_lines() {
    assert_eq!(
        tool_output_preview("one\ntwo\nthree", 1),
        ToolOutputPreview::Truncated {
            head: vec![],
            omitted: 3,
            tail: vec![],
        }
    );
}

#[test]
fn transcript_modal_widget_renders_empty_state_and_footer_without_show_all() {
    let _locale = locale_test_guard("en");
    let app_state = AppState::default();
    let state = TranscriptState::new();
    let body = render_transcript_text(&app_state, &state, 72, 8);

    assert!(body.contains("No messages yet."));
    assert!(body.contains("ctrl+o toggle"));
    assert!(body.contains("PgUp/PgDn page"));
    assert!(body.contains("Esc/q quit"));
    assert!(!body.contains("ctrl+e"));
    assert!(!body.contains("show all"));
}

#[test]
fn snapshot_transcript_modal_empty_footer() {
    let _locale = locale_test_guard("en");
    let app_state = AppState::default();
    let state = TranscriptState::new();

    insta::assert_snapshot!(
        "transcript_modal_empty_footer",
        render_transcript_text(&app_state, &state, 72, 8)
    );
}

#[test]
fn snapshot_transcript_modal_selected_tool_preview() {
    let _locale = locale_test_guard("en");
    let mut app_state = AppState::default();
    app_state
        .session
        .add_message(ChatMessage::user_text("user", "inspect src/lib.rs"));
    app_state.session.add_message(tool_use_message("call-1"));
    app_state.session.add_message(ChatMessage::tool_success(
        "tool-call-1",
        "Read",
        "pub fn alpha() {}\n\
         pub fn beta() {}\n\
         pub fn gamma() {}\n\
         pub fn delta() {}\n\
         pub fn epsilon() {}\n\
         pub fn zeta() {}",
    ));
    app_state.session.add_message(ChatMessage::assistant_text(
        "assistant",
        "Found the helpers.",
    ));
    let state = TranscriptState::new_with_anchor(Some(TranscriptCellId::tool("call-1")));

    insta::assert_snapshot!(
        "transcript_modal_selected_tool_preview",
        render_transcript_text(&app_state, &state, 84, 14)
    );
}

#[test]
fn snapshot_transcript_modal_expanded_thinking_cell() {
    let _locale = locale_test_guard("en");
    let mut app_state = AppState::default();
    app_state
        .session
        .add_message(ChatMessage::user_text("user", "bash ls"));
    app_state.session.add_message(ChatMessage {
        id: "thinking".into(),
        role: ChatRole::Assistant,
        content: MessageContent::Thinking {
            content: "The user wants me to run `ls` in the current working directory.\n\
                I should call the Bash tool and then summarize the result."
                .into(),
            duration_ms: Some(1300),
            reasoning_tokens: Some(15),
        },
        is_meta: false,
        created_at_ms: 0,
        is_compact_summary: false,
        is_visible_in_transcript_only: false,
        permission_mode: None,
    });
    app_state.session.add_message(ChatMessage::assistant_text(
        "assistant",
        "I'll list the current directory.",
    ));
    let state = TranscriptState::new_with_anchor(Some(TranscriptCellId::message(1, "thinking")));

    insta::assert_snapshot!(
        "transcript_modal_expanded_thinking_cell",
        render_transcript_text(&app_state, &state, 96, 12)
    );
}

#[test]
fn transcript_modal_collapsed_tool_keeps_header_and_head_tail_preview() {
    let _locale = locale_test_guard("en");
    let mut app_state = AppState::default();
    app_state.session.add_message(ChatMessage::tool_success(
        "tool-call-1",
        "Glob",
        "common/error/README.md\n\
         common/otel/README.md\n\
         retrieval/README.md\n\
         services/lsp/README.md\n\
         services/mcp-types/README.md\n\
         utils/file-search/README.md\n\
         utils/git/README.md\n\
         utils/shell-parser/README.md\n\
         utils/stdio-to-uds/README.md\n\
         utils/stream-parser/README.md\n\
         exec/apply-patch/tests/fixtures/scenarios/README.md\n\
         vercel-ai/README.md\n\
         core/system-reminder/README.md",
    ));
    let mut state = TranscriptState::new_with_anchor(Some(TranscriptCellId::tool("call-1")));
    state
        .collapsed_cell_ids
        .insert(TranscriptCellId::tool("call-1"));

    let body = render_transcript_text(&app_state, &state, 96, 14);

    assert!(body.contains("● Glob"));
    assert!(body.contains("└ common/error/README.md"));
    assert!(body.contains("common/otel/README.md"));
    assert!(body.contains("… +9 lines (ctrl+o to expand)"));
    assert!(body.contains("vercel-ai/README.md"));
    assert!(body.contains("core/system-reminder/README.md"));
    assert!(!body.contains("retrieval/README.md"));
}

#[test]
fn transcript_text_messages_are_full_and_not_expandable() {
    let _locale = locale_test_guard("en");
    let repeated = "one\ntwo\nthree\nfour\nfive\nsix";
    let mut app_state = AppState::default();
    app_state
        .session
        .add_message(ChatMessage::assistant_text("duplicate", repeated));
    app_state
        .session
        .add_message(ChatMessage::assistant_text("duplicate", repeated));
    let state = TranscriptState::new();

    let body = render_transcript_text(&app_state, &state, 84, 18);

    assert!(body.contains("six"));
    assert!(!body.contains(TRANSCRIPT_TRUNCATED_HINT));
    assert!(transcript_expandable_cell_ids(&app_state).is_empty());
}

#[test]
fn snapshot_transcript_modal_expanded_truncation_tail() {
    let _locale = locale_test_guard("en");
    let mut app_state = AppState::default();
    app_state.session.add_message(tool_use_message("call-1"));
    app_state.session.add_message(ChatMessage::tool_success(
        "tool-call-1",
        "Read",
        (0..=TRANSCRIPT_EXPANDED_CELL_LINE_CAP)
            .map(|i| format!("expanded-line-{i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    ));
    let mut state = TranscriptState::new_with_anchor(Some(TranscriptCellId::tool("call-1")));
    state.scroll = crate::state::transcript::TranscriptScrollPosition::Absolute(
        TRANSCRIPT_EXPANDED_CELL_LINE_CAP.saturating_sub(4),
    );

    insta::assert_snapshot!(
        "transcript_modal_expanded_truncation_tail",
        render_transcript_text(&app_state, &state, 84, 12)
    );
}

#[test]
fn snapshot_transcript_modal_streaming_tail() {
    let _locale = locale_test_guard("en");
    let mut app_state = AppState::default();
    app_state
        .session
        .add_message(ChatMessage::user_text("user", "summarize status"));
    let mut streaming = StreamingState::new();
    streaming.append_text("Working through the transcript pager changes.");
    streaming.reveal_all();
    app_state.ui.streaming = Some(streaming);
    let state = TranscriptState::new();

    insta::assert_snapshot!(
        "transcript_modal_streaming_tail",
        render_transcript_text(&app_state, &state, 84, 12)
    );
}

fn render_transcript_text(
    state: &AppState,
    transcript: &TranscriptState,
    width: u16,
    height: u16,
) -> String {
    let theme = Theme::default();
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    let mut layout = crate::widgets::TranscriptLayoutIndex::default();
    TranscriptStateWidget::new(state, transcript, &mut layout, UiStyles::new(&theme))
        .render(area, &mut buffer);
    buffer
        .content
        .chunks(width as usize)
        .map(|cells| cells.iter().map(ratatui::buffer::Cell::symbol).collect())
        .collect::<Vec<String>>()
        .join("\n")
}

fn projection_cells(messages: &[ChatMessage], show_system_reminders: bool) -> Vec<TranscriptCell> {
    transcript_projection(
        messages,
        TranscriptProjectionOptions {
            show_system_reminders,
        },
    )
    .cells
}

#[test]
fn transcript_projection_distinguishes_meta_and_regular_messages() {
    let mut meta = ChatMessage::system_text("meta", "system reminder");
    meta.is_meta = true;
    let messages = vec![meta, ChatMessage::user_text("user", "hello")];

    assert_eq!(
        projection_cells(&messages, false),
        vec![
            TranscriptCell::MetaPreview { index: 0 },
            TranscriptCell::Message { index: 1 },
        ]
    );
    assert_eq!(
        projection_cells(&messages, true),
        vec![
            TranscriptCell::Message { index: 0 },
            TranscriptCell::Message { index: 1 },
        ]
    );
}

#[test]
fn transcript_projection_groups_parallel_tool_runs() {
    let mut meta = ChatMessage::system_text("meta", "tool reminder");
    meta.is_meta = true;
    let messages = vec![
        tool_use_message("tool-1"),
        meta,
        tool_use_message("tool-2"),
        ChatMessage::assistant_text("done", "done"),
    ];

    assert_eq!(
        projection_cells(&messages, false),
        vec![
            TranscriptCell::ToolBatch {
                start: 0,
                end: 3,
                count: 3,
            },
            TranscriptCell::Message { index: 3 },
        ]
    );
}

#[test]
fn transcript_projection_pairs_tool_use_with_result_by_call_id() {
    let messages = vec![
        ChatMessage::user_text("user", "list files"),
        tool_use_message("call-1"),
        ChatMessage::assistant_text("between", "checking"),
        ChatMessage::tool_success("tool-call-1", "Read", "alpha\nbeta"),
    ];

    assert_eq!(
        projection_cells(&messages, false),
        vec![
            TranscriptCell::Message { index: 0 },
            TranscriptCell::ToolCall {
                invocation: Some(1),
                result: Some(3),
                call_id: Some("call-1".to_string()),
            },
            TranscriptCell::Message { index: 2 },
        ]
    );
}

#[test]
fn transcript_modal_widget_highlights_anchor_cell_and_keeps_it_expanded() {
    let mut app_state = AppState::default();
    app_state
        .session
        .add_message(ChatMessage::user_text("user", "list"));
    app_state.session.add_message(tool_use_message("call-1"));
    app_state.session.add_message(ChatMessage::tool_success(
        "tool-call-1",
        "Read",
        "alpha\nbeta",
    ));
    let state = TranscriptState::new_with_anchor(Some(TranscriptCellId::tool("call-1")));
    let body = render_transcript_text(&app_state, &state, 80, 12);

    assert!(body.contains("▶"));
    assert!(body.contains("Read"));
    assert!(body.contains("alpha"));
}

#[test]
fn transcript_modal_expands_tool_cells_by_default() {
    let _locale = locale_test_guard("en");
    let mut app_state = AppState::default();
    app_state.session.add_message(tool_use_message("old-call"));
    app_state.session.add_message(ChatMessage::tool_success(
        "tool-old-call",
        "Read",
        "old-alpha\nold-beta\nold-gamma\nold-delta\nold-epsilon\nold-zeta",
    ));
    app_state.session.add_message(tool_use_message("new-call"));
    app_state.session.add_message(ChatMessage::tool_success(
        "tool-new-call",
        "Read",
        "new-alpha\nnew-beta\nnew-gamma\nnew-delta\nnew-epsilon\nnew-zeta",
    ));
    let state = TranscriptState::new();
    let body = render_transcript_text(&app_state, &state, 80, 24);

    assert!(body.contains("old-alpha"));
    assert!(body.contains("new-alpha"));
    assert!(body.contains("old-zeta"));
    assert!(body.contains("new-zeta"));
    assert!(!body.contains(TRANSCRIPT_TRUNCATED_HINT));
}

#[test]
fn transcript_modal_caps_expanded_tool_result_lines() {
    let _locale = locale_test_guard("en");
    let mut app_state = AppState::default();
    app_state.session.add_message(tool_use_message("call-1"));
    app_state.session.add_message(ChatMessage::tool_success(
        "tool-call-1",
        "Read",
        (0..=TRANSCRIPT_EXPANDED_CELL_LINE_CAP)
            .map(|i| format!("line-{i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    ));
    let state = TranscriptState::new_with_anchor(Some(TranscriptCellId::tool("call-1")));

    let body = render_transcript_text(
        &app_state,
        &state,
        80,
        (TRANSCRIPT_EXPANDED_CELL_LINE_CAP + 8) as u16,
    );

    assert!(body.contains("line-0"));
    assert!(!body.contains(&format!("line-{TRANSCRIPT_EXPANDED_CELL_LINE_CAP}")));
    assert!(body.contains("output truncated in UI"));
}

#[test]
fn transcript_projection_collapses_consecutive_same_hook_by_default() {
    let messages = vec![
        hook_success("hook-1", "PostToolUse"),
        hook_error("hook-2", "PostToolUse"),
        ChatMessage::assistant_text("done", "done"),
    ];

    assert_eq!(
        projection_cells(&messages, false),
        vec![
            TranscriptCell::HookBatch {
                start: 0,
                end: 2,
                count: 2,
                hook_name: "PostToolUse".to_string(),
                has_error: true,
            },
            TranscriptCell::Message { index: 2 },
        ]
    );
}

#[test]
fn transcript_projection_keeps_hook_details_when_showing_all() {
    let messages = vec![
        hook_success("hook-1", "PostToolUse"),
        hook_error("hook-2", "PostToolUse"),
    ];

    assert_eq!(
        projection_cells(&messages, true),
        vec![
            TranscriptCell::Message { index: 0 },
            TranscriptCell::Message { index: 1 },
        ]
    );
}

#[test]
fn transcript_projection_does_not_merge_different_hooks() {
    let messages = vec![
        hook_success("hook-1", "PreToolUse"),
        hook_success("hook-2", "PostToolUse"),
    ];

    assert_eq!(
        projection_cells(&messages, false),
        vec![
            TranscriptCell::Message { index: 0 },
            TranscriptCell::Message { index: 1 },
        ]
    );
}

#[test]
fn transcript_projection_collapses_background_bash_notifications_by_default() {
    let messages = vec![
        task_notification_message(
            "task-1",
            "tb11111111",
            "completed",
            "Background command \"cargo test\" completed",
        ),
        task_notification_message(
            "task-2",
            "tb22222222",
            "completed",
            "Background command \"just fmt\" completed",
        ),
        ChatMessage::assistant_text("done", "done"),
    ];

    assert_eq!(
        projection_cells(&messages, false),
        vec![
            TranscriptCell::TaskNotificationBatch {
                start: 0,
                end: 2,
                count: 2,
                kind: TaskNotificationBatchKind::BackgroundBashCompleted,
            },
            TranscriptCell::Message { index: 2 },
        ]
    );
}

#[test]
fn transcript_projection_keeps_task_notification_details_when_showing_all() {
    let messages = vec![
        task_notification_message(
            "task-1",
            "tb11111111",
            "completed",
            "Background command \"cargo test\" completed",
        ),
        task_notification_message(
            "task-2",
            "tb22222222",
            "completed",
            "Background command \"just fmt\" completed",
        ),
    ];

    assert_eq!(
        projection_cells(&messages, true),
        vec![
            TranscriptCell::TaskNotification {
                index: 0,
                summary: "Background command \"cargo test\" completed".to_string(),
                tone: TaskNotificationTone::Completed,
            },
            TranscriptCell::TaskNotification {
                index: 1,
                summary: "Background command \"just fmt\" completed".to_string(),
                tone: TaskNotificationTone::Completed,
            },
        ]
    );
}

#[test]
fn transcript_projection_does_not_batch_failed_task_notifications() {
    let messages = vec![
        task_notification_message(
            "task-1",
            "tb11111111",
            "failed",
            "Background command \"cargo test\" failed",
        ),
        task_notification_message(
            "task-2",
            "tb22222222",
            "failed",
            "Background command \"just fmt\" failed",
        ),
    ];

    assert_eq!(
        projection_cells(&messages, false),
        vec![
            TranscriptCell::TaskNotification {
                index: 0,
                summary: "Background command \"cargo test\" failed".to_string(),
                tone: TaskNotificationTone::Failed,
            },
            TranscriptCell::TaskNotification {
                index: 1,
                summary: "Background command \"just fmt\" failed".to_string(),
                tone: TaskNotificationTone::Failed,
            },
        ]
    );
}

#[test]
fn transcript_projection_collapses_teammate_shutdown_notifications() {
    let messages = vec![
        task_notification_message(
            "task-1",
            "tt11111111",
            "completed",
            "teammate alpha shut down",
        ),
        task_notification_message(
            "task-2",
            "tt22222222",
            "completed",
            "teammate beta shut down",
        ),
    ];

    assert_eq!(
        projection_cells(&messages, false),
        vec![TranscriptCell::TaskNotificationBatch {
            start: 0,
            end: 2,
            count: 2,
            kind: TaskNotificationBatchKind::TeammateShutdown,
        }]
    );
}

#[test]
fn transcript_projection_parses_wrapped_task_notification_before_meta_preview() {
    let mut wrapped = ChatMessage::system_text(
        "task-1",
        format!(
            "A background agent completed a task:\n{}",
            task_notification_xml(
                "ta11111111",
                "completed",
                "Agent \"Investigate auth bug\" completed",
            )
        ),
    );
    wrapped.is_meta = true;
    let messages = vec![wrapped];

    assert_eq!(
        projection_cells(&messages, false),
        vec![TranscriptCell::TaskNotification {
            index: 0,
            summary: "Agent \"Investigate auth bug\" completed".to_string(),
            tone: TaskNotificationTone::Completed,
        }]
    );
}

#[test]
fn active_transcript_cell_prioritizes_streaming_over_busy_spinner() {
    let mut streaming = StreamingState::new();
    streaming.append_text("hello");
    streaming.reveal_all();
    let tools = vec![tool_execution(ToolStatus::Running)];

    assert_eq!(
        active_transcript_cell(Some(&streaming), true, &tools),
        Some(ActiveTranscriptCell::Streaming(StreamingTailView {
            blocks: vec![
                StreamingTailBlock::AssistantText("hello"),
                StreamingTailBlock::Cursor,
            ],
        }))
    );
    assert_eq!(
        active_transcript_cell(None, true, &tools),
        Some(ActiveTranscriptCell::BusySpinner)
    );
    assert_eq!(
        active_transcript_cell(None, true, &[tool_execution(ToolStatus::Completed)]),
        None
    );
}

#[test]
fn transcript_presentation_appends_active_streaming_after_committed_cells() {
    let streaming = StreamingState::new();
    let messages = vec![ChatMessage::user_text("user", "hello")];
    let presentation = transcript_presentation(TranscriptPresentationInput {
        messages: &messages,
        options: TranscriptProjectionOptions {
            show_system_reminders: false,
        },
        streaming: Some(&streaming),
        show_thinking: true,
        tool_executions: &[],
    });

    assert_eq!(
        presentation.cells,
        vec![
            TranscriptSourceCell::Committed(TranscriptCell::Message { index: 0 }),
            TranscriptSourceCell::Active(ActiveTranscriptCell::Streaming(StreamingTailView {
                blocks: Vec::new(),
            })),
        ]
    );
}

#[test]
fn transcript_presentation_appends_busy_spinner_when_tools_are_active() {
    let messages = vec![ChatMessage::assistant_text("done", "done")];
    let tools = vec![tool_execution(ToolStatus::Queued)];
    let presentation = transcript_presentation(TranscriptPresentationInput {
        messages: &messages,
        options: TranscriptProjectionOptions {
            show_system_reminders: false,
        },
        streaming: None,
        show_thinking: true,
        tool_executions: &tools,
    });

    assert_eq!(
        presentation.cells,
        vec![
            TranscriptSourceCell::Committed(TranscriptCell::Message { index: 0 }),
            TranscriptSourceCell::Active(ActiveTranscriptCell::BusySpinner),
        ]
    );
}

#[test]
fn transcript_presentation_omits_active_cell_when_idle() {
    let messages = vec![ChatMessage::assistant_text("done", "done")];
    let tools = vec![tool_execution(ToolStatus::Completed)];
    let presentation = transcript_presentation(TranscriptPresentationInput {
        messages: &messages,
        options: TranscriptProjectionOptions {
            show_system_reminders: false,
        },
        streaming: None,
        show_thinking: true,
        tool_executions: &tools,
    });

    assert_eq!(
        presentation.cells,
        vec![TranscriptSourceCell::Committed(TranscriptCell::Message {
            index: 0
        })]
    );
}

fn hook_success(id: &str, hook_name: &str) -> ChatMessage {
    ChatMessage {
        id: id.to_string(),
        role: ChatRole::System,
        content: MessageContent::HookSuccess {
            hook_name: hook_name.to_string(),
            output: "ok".to_string(),
        },
        is_meta: false,
        created_at_ms: 0,
        is_compact_summary: false,
        is_visible_in_transcript_only: false,
        permission_mode: None,
    }
}

fn hook_error(id: &str, hook_name: &str) -> ChatMessage {
    ChatMessage {
        id: id.to_string(),
        role: ChatRole::System,
        content: MessageContent::HookNonBlockingError {
            hook_name: hook_name.to_string(),
            error: "failed".to_string(),
        },
        is_meta: false,
        created_at_ms: 0,
        is_compact_summary: false,
        is_visible_in_transcript_only: false,
        permission_mode: None,
    }
}

fn task_notification_message(id: &str, task_id: &str, status: &str, summary: &str) -> ChatMessage {
    ChatMessage::user_text(id, task_notification_xml(task_id, status, summary))
}

fn task_notification_xml(task_id: &str, status: &str, summary: &str) -> String {
    format!(
        "<task-notification>\n\
         <task-id>{task_id}</task-id>\n\
         <status>{status}</status>\n\
         <summary>{summary}</summary>\n\
         </task-notification>"
    )
}

fn tool_use_message(id: &str) -> ChatMessage {
    ChatMessage {
        id: id.to_string(),
        role: ChatRole::Assistant,
        content: MessageContent::ToolUse {
            tool_name: "Read".to_string(),
            call_id: id.to_string(),
            input_preview: "{}".to_string(),
            status: ToolUseStatus::Running,
        },
        is_meta: false,
        created_at_ms: 0,
        is_compact_summary: false,
        is_visible_in_transcript_only: false,
        permission_mode: None,
    }
}

fn tool_execution(status: ToolStatus) -> ToolExecution {
    ToolExecution {
        call_id: "call".to_string(),
        name: "Read".to_string(),
        status,
        started_at: std::time::Instant::now(),
        completed_at: None,
        description: None,
        streaming_input: None,
    }
}
