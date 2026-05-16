//! Transcript overlay presentation.

use coco_keybindings::KeybindingAction;
use coco_subagent::ParsedTaskNotification;
use coco_subagent::TaskNotificationStatus;
use coco_subagent::parse_task_notification;
use ratatui::prelude::Color;

use crate::i18n::t;
use crate::keybinding_bridge::KeybindingContext as TuiContext;
use crate::presentation::pager;
use crate::presentation::streaming::StreamingTailInput;
use crate::presentation::streaming::StreamingTailView;
use crate::presentation::streaming::streaming_tail_view;
use crate::presentation::styles::UiStyles;
use crate::state::AppState;
use crate::state::overlay::TranscriptOverlay;
use crate::state::session::ChatMessage;
use crate::state::session::MessageContent;
use crate::state::session::ToolExecution;
use crate::state::session::ToolStatus;
use crate::state::ui::StreamingState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TranscriptCell {
    MetaPreview {
        index: usize,
    },
    Message {
        index: usize,
    },
    ToolBatch {
        start: usize,
        end: usize,
        count: usize,
    },
    HookBatch {
        start: usize,
        end: usize,
        count: usize,
        hook_name: String,
        has_error: bool,
    },
    TaskNotification {
        index: usize,
        summary: String,
        tone: TaskNotificationTone,
    },
    TaskNotificationBatch {
        start: usize,
        end: usize,
        count: usize,
        kind: TaskNotificationBatchKind,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TranscriptSourceCell<'a> {
    Committed(TranscriptCell),
    Active(ActiveTranscriptCell<'a>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActiveTranscriptCell<'a> {
    Streaming(StreamingTailView<'a>),
    BusySpinner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskNotificationTone {
    Completed,
    Failed,
    Killed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskNotificationBatchKind {
    BackgroundBashCompleted,
    TeammateShutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TranscriptProjectionOptions {
    pub show_system_reminders: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptProjection {
    pub cells: Vec<TranscriptCell>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TranscriptPresentationInput<'a> {
    pub messages: &'a [ChatMessage],
    pub options: TranscriptProjectionOptions,
    pub streaming: Option<&'a StreamingState>,
    pub show_thinking: bool,
    pub tool_executions: &'a [ToolExecution],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptPresentation<'a> {
    pub cells: Vec<TranscriptSourceCell<'a>>,
}

pub(crate) fn transcript_projection(
    messages: &[ChatMessage],
    options: TranscriptProjectionOptions,
) -> TranscriptProjection {
    let show_system_reminders = options.show_system_reminders;
    let mut cells = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        let msg = &messages[i];
        if let Some(batch) = task_notification_batch(messages, i, show_system_reminders) {
            i = batch.end();
            cells.push(batch);
            continue;
        }

        if let Some(notification) = task_notification_cell(&messages[i].content, i) {
            cells.push(notification);
            i += 1;
            continue;
        }

        if msg.is_meta && !show_system_reminders {
            cells.push(TranscriptCell::MetaPreview { index: i });
            i += 1;
            continue;
        }

        if !show_system_reminders && let Some(batch) = hook_batch(messages, i) {
            i = batch.end();
            cells.push(batch);
            continue;
        }

        let batch_end = tool_batch_end(messages, i);
        if batch_end > i + 1 {
            cells.push(TranscriptCell::ToolBatch {
                start: i,
                end: batch_end,
                count: batch_end - i,
            });
            i = batch_end;
            continue;
        }

        cells.push(TranscriptCell::Message { index: i });
        i += 1;
    }
    TranscriptProjection { cells }
}

pub(crate) fn transcript_presentation(
    input: TranscriptPresentationInput<'_>,
) -> TranscriptPresentation<'_> {
    let mut cells = transcript_projection(input.messages, input.options)
        .cells
        .into_iter()
        .map(TranscriptSourceCell::Committed)
        .collect::<Vec<_>>();
    if let Some(active) =
        active_transcript_cell(input.streaming, input.show_thinking, input.tool_executions)
    {
        cells.push(TranscriptSourceCell::Active(active));
    }
    TranscriptPresentation { cells }
}

pub(crate) fn active_transcript_cell<'a>(
    streaming: Option<&'a StreamingState>,
    show_thinking: bool,
    tool_executions: &[ToolExecution],
) -> Option<ActiveTranscriptCell<'a>> {
    if streaming.is_some() {
        return streaming.map(|streaming| {
            ActiveTranscriptCell::Streaming(streaming_tail_view(StreamingTailInput {
                streaming,
                show_thinking,
            }))
        });
    }
    if tool_executions
        .iter()
        .any(|t| matches!(t.status, ToolStatus::Queued | ToolStatus::Running))
    {
        return Some(ActiveTranscriptCell::BusySpinner);
    }
    None
}

fn tool_batch_end(messages: &[ChatMessage], start: usize) -> usize {
    let is_tool_use = |m: &ChatMessage| matches!(m.content, MessageContent::ToolUse { .. });
    if !is_tool_use(&messages[start]) {
        return start + 1;
    }
    let mut end = start + 1;
    while end < messages.len() {
        let next = &messages[end];
        if is_tool_use(next) || next.is_meta {
            end += 1;
        } else {
            break;
        }
    }
    end
}

fn hook_batch(messages: &[ChatMessage], start: usize) -> Option<TranscriptCell> {
    let name = hook_name(&messages[start].content)?;
    let mut end = start + 1;
    let mut has_error = hook_has_error(&messages[start].content);
    while end < messages.len() {
        let next = &messages[end].content;
        if hook_name(next) != Some(name) {
            break;
        }
        has_error |= hook_has_error(next);
        end += 1;
    }
    let count = end - start;
    if count <= 1 {
        return None;
    }
    Some(TranscriptCell::HookBatch {
        start,
        end,
        count,
        hook_name: name.to_string(),
        has_error,
    })
}

fn hook_name(content: &MessageContent) -> Option<&str> {
    match content {
        MessageContent::HookSuccess { hook_name, .. }
        | MessageContent::HookNonBlockingError { hook_name, .. }
        | MessageContent::HookBlockingError { hook_name, .. }
        | MessageContent::HookCancelled { hook_name }
        | MessageContent::HookSystemMessage { hook_name, .. }
        | MessageContent::HookAdditionalContext { hook_name, .. }
        | MessageContent::HookStoppedContinuation { hook_name, .. }
        | MessageContent::HookAsyncResponse { hook_name, .. } => Some(hook_name.as_str()),
        _ => None,
    }
}

fn hook_has_error(content: &MessageContent) -> bool {
    matches!(
        content,
        MessageContent::HookNonBlockingError { .. }
            | MessageContent::HookBlockingError { .. }
            | MessageContent::HookStoppedContinuation { .. }
    )
}

fn task_notification_batch(
    messages: &[ChatMessage],
    start: usize,
    show_system_reminders: bool,
) -> Option<TranscriptCell> {
    if show_system_reminders {
        return None;
    }
    let first = parsed_task_notification(&messages[start].content)?;
    let kind = task_notification_batch_kind(&first)?;
    let mut end = start + 1;
    while end < messages.len() {
        let Some(next) = parsed_task_notification(&messages[end].content) else {
            break;
        };
        if task_notification_batch_kind(&next) != Some(kind) {
            break;
        }
        end += 1;
    }

    let count = end - start;
    if count <= 1 {
        return None;
    }
    Some(TranscriptCell::TaskNotificationBatch {
        start,
        end,
        count,
        kind,
    })
}

fn task_notification_cell(content: &MessageContent, index: usize) -> Option<TranscriptCell> {
    let notification = parsed_task_notification(content)?;
    Some(TranscriptCell::TaskNotification {
        index,
        summary: notification.summary,
        tone: task_notification_tone(notification.status),
    })
}

fn parsed_task_notification(content: &MessageContent) -> Option<ParsedTaskNotification> {
    let text = match content {
        MessageContent::Text(text)
        | MessageContent::SystemText(text)
        | MessageContent::AgentNotification { summary: text, .. }
        | MessageContent::TeammateMessage { content: text, .. } => text.as_str(),
        MessageContent::Attachment { preview, .. } => preview.as_str(),
        _ => return None,
    };
    parse_embedded_task_notification(text)
}

fn parse_embedded_task_notification(text: &str) -> Option<ParsedTaskNotification> {
    parse_task_notification(text).or_else(|| {
        let start = text.find("<task-notification>")?;
        parse_task_notification(&text[start..])
    })
}

fn task_notification_tone(status: TaskNotificationStatus) -> TaskNotificationTone {
    match status {
        TaskNotificationStatus::Completed => TaskNotificationTone::Completed,
        TaskNotificationStatus::Failed => TaskNotificationTone::Failed,
        TaskNotificationStatus::Killed => TaskNotificationTone::Killed,
        _ => TaskNotificationTone::Unknown,
    }
}

fn task_notification_batch_kind(
    notification: &ParsedTaskNotification,
) -> Option<TaskNotificationBatchKind> {
    if notification.status != TaskNotificationStatus::Completed {
        return None;
    }
    if is_background_bash_notification(notification) {
        return Some(TaskNotificationBatchKind::BackgroundBashCompleted);
    }
    if is_teammate_shutdown_notification(notification) {
        return Some(TaskNotificationBatchKind::TeammateShutdown);
    }
    None
}

fn is_background_bash_notification(notification: &ParsedTaskNotification) -> bool {
    notification.summary.starts_with("Background command ")
        || notification.task_id.starts_with("tb")
}

fn is_teammate_shutdown_notification(notification: &ParsedTaskNotification) -> bool {
    notification.task_id.starts_with("tt")
}

impl TranscriptCell {
    fn end(&self) -> usize {
        match self {
            Self::MetaPreview { index } | Self::Message { index } => index + 1,
            Self::ToolBatch { end, .. }
            | Self::HookBatch { end, .. }
            | Self::TaskNotificationBatch { end, .. } => *end,
            Self::TaskNotification { index, .. } => index + 1,
        }
    }
}

pub(crate) fn transcript_overlay_content(
    state: &AppState,
    overlay: &TranscriptOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    let title = t!("transcript.title").to_string();
    let mut chat = crate::widgets::ChatWidget::new(&state.session.messages, styles)
        .show_thinking(true)
        .show_system_reminders(overlay.show_all)
        .tool_executions(&state.session.tool_executions)
        .syntax_highlighting(state.ui.display_settings.syntax_highlighting)
        .kb_handle(&state.ui.kb_handle);
    if !state.ui.collapsed_tools.is_empty() {
        chat = chat.collapsed_tools(&state.ui.collapsed_tools);
    }

    let lines = chat.build_lines_owned();
    let window = pager::pager_window(lines.len(), overlay.scroll, lines.len());
    let body_text = lines
        .get(window.range())
        .unwrap_or_default()
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let body_text = if body_text.is_empty() {
        t!("transcript.empty").to_string()
    } else {
        body_text
    };

    let toggle_chord = state
        .ui
        .kb_handle
        .display_for(&KeybindingAction::AppToggleTranscript, TuiContext::Chat)
        .unwrap_or_else(|| "ctrl+o".to_string());
    let show_all_chord = state
        .ui
        .kb_handle
        .display_for(
            &KeybindingAction::TranscriptToggleShowAll,
            TuiContext::Scrollable,
        )
        .unwrap_or_else(|| "ctrl+e".to_string());
    let show_all_label = if overlay.show_all {
        t!("transcript.hint_show_all_on").to_string()
    } else {
        t!("transcript.hint_show_all_off").to_string()
    };
    let footer = t!(
        "transcript.hint_footer",
        toggle = toggle_chord.as_str(),
        show_all_chord = show_all_chord.as_str(),
        show_all = show_all_label.as_str(),
    )
    .to_string();

    (title, format!("{body_text}\n\n{footer}"), styles.primary())
}

#[cfg(test)]
#[path = "transcript.test.rs"]
mod tests;
