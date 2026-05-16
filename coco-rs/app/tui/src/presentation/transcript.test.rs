use super::*;
use pretty_assertions::assert_eq;

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
use crate::state::ui::StreamingState;
use crate::theme::Theme;

#[test]
fn transcript_overlay_content_renders_empty_state_and_show_all_footer() {
    let _locale = locale_test_guard("en");
    let state = AppState::default();
    let theme = Theme::default();
    let mut overlay = TranscriptOverlay::new();
    overlay.show_all = false;
    overlay.scroll = -5;

    let (title, body, border) = transcript_overlay_content(&state, &overlay, UiStyles::new(&theme));

    assert_eq!(title, " Transcript ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("No messages yet."));
    assert!(body.contains("ctrl+o to toggle"));
    assert!(body.contains("show all"));
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
