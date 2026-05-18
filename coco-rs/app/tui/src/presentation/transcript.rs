//! Transcript state presentation.

use std::collections::VecDeque;

use coco_subagent::ParsedTaskNotification;
use coco_subagent::TaskNotificationStatus;
use coco_subagent::parse_task_notification;

use crate::presentation::streaming::StreamingTailInput;
use crate::presentation::streaming::StreamingTailView;
use crate::presentation::streaming::streaming_tail_view;
use crate::state::AppState;
use crate::state::session::ChatMessage;
use crate::state::session::MessageContent;
use crate::state::session::ToolExecution;
use crate::state::session::ToolStatus;
use crate::state::transcript::TranscriptCellId;
use crate::state::ui::StreamingState;

pub(crate) const TRANSCRIPT_COLLAPSED_PREVIEW_LINES: usize = 5;
pub(crate) const TRANSCRIPT_EXPANDED_CELL_LINE_CAP: usize = 2_000;
pub(crate) const TRANSCRIPT_LINE_CHAR_CAP: usize = 512;
pub(crate) const TRANSCRIPT_TRUNCATED_HINT: &str = "… output truncated in UI";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolOutputPreview<'a> {
    Empty,
    Full(Vec<&'a str>),
    Truncated {
        head: Vec<&'a str>,
        omitted: usize,
        tail: Vec<&'a str>,
    },
}

pub(crate) fn tool_output_preview(output: &str, max_rows: usize) -> ToolOutputPreview<'_> {
    if max_rows == 0 {
        return ToolOutputPreview::Empty;
    }

    let visible_rows = max_rows.saturating_sub(1);
    let head_limit = visible_rows / 2;
    let tail_limit = visible_rows.saturating_sub(head_limit);
    let mut short = Vec::with_capacity(max_rows);
    let mut head = Vec::with_capacity(head_limit);
    let mut tail = VecDeque::with_capacity(tail_limit);
    let mut total = 0usize;

    for line in output.lines() {
        if total < max_rows {
            short.push(line);
        }
        if total < head_limit {
            head.push(line);
        } else if tail_limit > 0 {
            if tail.len() == tail_limit {
                tail.pop_front();
            }
            tail.push_back(line);
        }
        total += 1;
    }

    if total == 0 {
        return ToolOutputPreview::Empty;
    }
    if total <= max_rows {
        return ToolOutputPreview::Full(short);
    }

    ToolOutputPreview::Truncated {
        omitted: total.saturating_sub(head.len() + tail.len()),
        head,
        tail: tail.into_iter().collect(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TranscriptCell {
    MetaPreview {
        index: usize,
    },
    Message {
        index: usize,
    },
    ToolCall {
        invocation: Option<usize>,
        result: Option<usize>,
        call_id: Option<String>,
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
pub(crate) struct TranscriptPresentationInput<'msg, 'state> {
    /// Source slice the projection walks. `'msg` is decoupled from
    /// `'state` so callers can pass a freshly-derived `Vec<ChatMessage>`
    /// (e.g. `state.session.transcript_messages()`) by reference without
    /// pinning the resulting `TranscriptPresentation` to that
    /// temporary's lifetime. Only the streaming view in the output
    /// carries an inward borrow; cells themselves are owned.
    pub messages: &'msg [ChatMessage],
    pub options: TranscriptProjectionOptions,
    pub streaming: Option<&'state StreamingState>,
    pub show_thinking: bool,
    pub tool_executions: &'state [ToolExecution],
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
    let mut consumed = vec![false; messages.len()];
    let mut i = 0;
    while i < messages.len() {
        if consumed[i] {
            i += 1;
            continue;
        }
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
        if batch_end > i + 1 && !tool_batch_has_results(messages, &consumed, i, batch_end) {
            cells.push(TranscriptCell::ToolBatch {
                start: i,
                end: batch_end,
                count: batch_end - i,
            });
            i = batch_end;
            continue;
        }

        if let MessageContent::ToolUse {
            tool_name, call_id, ..
        } = &msg.content
        {
            let result = find_tool_result(messages, &consumed, i + 1, call_id, tool_name);
            if let Some(result) = result {
                consumed[result] = true;
            }
            cells.push(TranscriptCell::ToolCall {
                invocation: Some(i),
                result,
                call_id: Some(call_id.clone()),
            });
            i += 1;
            continue;
        }

        if is_tool_result(&msg.content) {
            cells.push(TranscriptCell::ToolCall {
                invocation: None,
                result: Some(i),
                call_id: call_id_from_tool_result_message_id(&msg.id),
            });
            i += 1;
            continue;
        }

        cells.push(TranscriptCell::Message { index: i });
        i += 1;
    }
    TranscriptProjection { cells }
}

pub(crate) fn transcript_presentation<'msg, 'state>(
    input: TranscriptPresentationInput<'msg, 'state>,
) -> TranscriptPresentation<'state> {
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

fn tool_batch_has_results(
    messages: &[ChatMessage],
    consumed: &[bool],
    start: usize,
    end: usize,
) -> bool {
    messages[start..end].iter().any(|msg| {
        let MessageContent::ToolUse {
            tool_name, call_id, ..
        } = &msg.content
        else {
            return false;
        };
        find_tool_result(messages, consumed, end, call_id, tool_name).is_some()
    })
}

fn find_tool_result(
    messages: &[ChatMessage],
    consumed: &[bool],
    start: usize,
    call_id: &str,
    tool_name: &str,
) -> Option<usize> {
    let exact_id = format!("tool-{call_id}");
    for i in start..messages.len() {
        if consumed[i] {
            continue;
        }
        let msg = &messages[i];
        if msg.id == exact_id && is_tool_result(&msg.content) {
            return Some(i);
        }
    }

    let mut skipped_tool_uses = 0usize;
    for i in start..messages.len() {
        if consumed[i] {
            continue;
        }
        match &messages[i].content {
            MessageContent::ToolUse { .. } => {
                skipped_tool_uses += 1;
                if skipped_tool_uses > 0 {
                    break;
                }
            }
            content if tool_result_name(content) == Some(tool_name) => return Some(i),
            content if is_tool_result(content) => break,
            content if is_turn_boundary(content) => break,
            _ => {}
        }
    }
    None
}

fn is_turn_boundary(content: &MessageContent) -> bool {
    matches!(
        content,
        MessageContent::Text(_) | MessageContent::AssistantText(_)
    )
}

fn is_tool_result(content: &MessageContent) -> bool {
    matches!(
        content,
        MessageContent::ToolSuccess { .. }
            | MessageContent::ToolError { .. }
            | MessageContent::ToolRejected { .. }
            | MessageContent::ToolCanceled { .. }
    )
}

fn tool_result_name(content: &MessageContent) -> Option<&str> {
    match content {
        MessageContent::ToolSuccess { tool_name, .. }
        | MessageContent::ToolError { tool_name, .. }
        | MessageContent::ToolRejected { tool_name, .. }
        | MessageContent::ToolCanceled { tool_name } => Some(tool_name.as_str()),
        _ => None,
    }
}

fn call_id_from_tool_result_message_id(id: &str) -> Option<String> {
    id.strip_prefix("tool-")
        .filter(|rest| !rest.is_empty())
        .map(ToString::to_string)
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
            Self::ToolCall {
                invocation, result, ..
            } => invocation
                .iter()
                .chain(result.iter())
                .copied()
                .max()
                .map(|idx| idx + 1)
                .unwrap_or(0),
            Self::ToolBatch { end, .. }
            | Self::HookBatch { end, .. }
            | Self::TaskNotificationBatch { end, .. } => *end,
            Self::TaskNotification { index, .. } => index + 1,
        }
    }

    pub(crate) fn cell_id(&self, messages: &[ChatMessage]) -> Option<TranscriptCellId> {
        match self {
            Self::ToolCall {
                call_id: Some(call_id),
                ..
            } => Some(TranscriptCellId::tool(call_id.clone())),
            Self::ToolCall {
                invocation: Some(index),
                ..
            }
            | Self::ToolCall {
                result: Some(index),
                ..
            }
            | Self::MetaPreview { index }
            | Self::Message { index }
            | Self::TaskNotification { index, .. } => Some(TranscriptCellId::message(
                *index,
                messages.get(*index)?.id.clone(),
            )),
            Self::ToolCall { .. } => None,
            Self::ToolBatch { start, end, .. } => Some(TranscriptCellId::tool_batch(*start, *end)),
            Self::HookBatch { start, end, .. } => Some(TranscriptCellId::hook_batch(*start, *end)),
            Self::TaskNotificationBatch { start, end, .. } => {
                Some(TranscriptCellId::task_notification_batch(*start, *end))
            }
        }
    }
}

impl<'a> TranscriptSourceCell<'a> {
    pub(crate) fn cell_id(&self, messages: &[ChatMessage]) -> Option<TranscriptCellId> {
        match self {
            Self::Committed(cell) => cell.cell_id(messages),
            Self::Active(_) => Some(TranscriptCellId::ActiveTail),
        }
    }

    pub(crate) fn is_expandable(&self, messages: &[ChatMessage]) -> bool {
        match self {
            Self::Committed(TranscriptCell::ToolCall { .. }) => true,
            Self::Committed(TranscriptCell::Message { index }) => messages
                .get(*index)
                .is_some_and(|message| message_content_is_expandable(&message.content)),
            Self::Committed(_) | Self::Active(_) => false,
        }
    }
}

fn message_content_is_expandable(content: &MessageContent) -> bool {
    match content {
        MessageContent::Thinking { content, .. } => !content.is_empty(),
        MessageContent::ToolSuccess { .. }
        | MessageContent::ToolError { .. }
        | MessageContent::ToolRejected { .. }
        | MessageContent::ToolCanceled { .. } => true,
        MessageContent::Text(_)
        | MessageContent::AssistantText(_)
        | MessageContent::SystemText(_) => false,
        _ => false,
    }
}

pub(crate) fn transcript_expandable_cell_ids(state: &AppState) -> Vec<TranscriptCellId> {
    // Source from the merged view so engine-derived cells (the bulk of
    // the live transcript after Commit 2) participate in the
    // expandable-cell list. Indices inside `TranscriptCell` are
    // computed against the SAME messages slice that the caller uses
    // for `cell_id` lookups — keep both in sync.
    let messages = state.session.transcript_messages();
    transcript_presentation(TranscriptPresentationInput {
        messages: &messages,
        options: TranscriptProjectionOptions {
            show_system_reminders: true,
        },
        streaming: state.ui.streaming.as_ref(),
        show_thinking: true,
        tool_executions: &state.session.tool_executions,
    })
    .cells
    .into_iter()
    .filter(|cell| cell.is_expandable(&messages))
    .filter_map(|cell| cell.cell_id(&messages))
    .collect()
}

pub(crate) fn latest_expandable_cell_id(state: &AppState) -> Option<TranscriptCellId> {
    transcript_expandable_cell_ids(state)
        .into_iter()
        .next_back()
}

/// Build a `TranscriptPresentation` from a caller-supplied messages
/// slice — the entry point for everything that wants to render the
/// chat transcript (typically the Ctrl+O modal). The caller is
/// responsible for sourcing the slice from
/// `state.session.transcript_messages()` (or an equivalent merged view)
/// so engine-derived cells participate; passing `state.session.messages`
/// directly works but is now mostly empty.
///
/// The `messages` lifetime is independent of `'state` so callers can
/// pass a slice tied to a local `Vec<ChatMessage>`: the returned
/// `TranscriptPresentation` only borrows from `state` (via the
/// streaming-tail view) — cells themselves are owned, so they survive
/// the messages slice being dropped.
pub(crate) fn transcript_presentation_with_messages<'state>(
    state: &'state AppState,
    messages: &[ChatMessage],
) -> TranscriptPresentation<'state> {
    transcript_presentation(TranscriptPresentationInput {
        messages,
        options: TranscriptProjectionOptions {
            show_system_reminders: true,
        },
        streaming: state.ui.streaming.as_ref(),
        show_thinking: true,
        tool_executions: &state.session.tool_executions,
    })
}

#[cfg(test)]
#[path = "transcript.test.rs"]
mod tests;
