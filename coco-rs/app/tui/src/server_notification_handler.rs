//! Server notification handler — processes protocol events from the agent loop.
//!
//! Architecture: this module receives `CoreEvent` from the agent loop (via
//! `handle_core_event`), translates each variant into one or more internal
//! `TuiNotification`s (the TUI's pragmatic state-update type), and applies
//! them via `handle_tui_notification`. This keeps the TUI's state mutation
//! logic stable while letting it consume the 3-layer CoreEvent protocol.

use coco_types::AgentStreamEvent;
use coco_types::CoreEvent;
use coco_types::ServerNotification;
use coco_types::TuiOnlyEvent;

use crate::state::AppState;
use crate::state::session::ChatMessage;
use crate::state::session::McpServerStatus;
use crate::state::session::SubagentInstance;
use crate::state::session::SubagentStatus;
use crate::state::session::TokenUsage;
use crate::state::ui::Toast;

/// Internal TUI notification type.
///
/// This is the pragmatic state-mutation type used inside the TUI.
/// External `CoreEvent`s are translated into zero or more `TuiNotification`s
/// via `core_event_to_tui_notifications()`.
#[derive(Debug, Clone)]
pub enum TuiNotification {
    /// Agent turn started.
    TurnStarted { turn_number: i32 },
    /// Agent turn completed.
    TurnCompleted { usage: TokenUsage },
    /// Agent turn failed.
    TurnFailed { error: String },
    /// Text content delta.
    TextDelta { delta: String },
    /// Thinking content delta.
    ThinkingDelta { delta: String },
    /// Tool use queued (before execution).
    ToolUseQueued {
        call_id: String,
        name: String,
        input_preview: String,
    },
    /// Tool use completed.
    ToolUseCompleted {
        call_id: String,
        output: String,
        is_error: bool,
    },
    /// Subagent spawned.
    SubagentSpawned {
        agent_id: String,
        agent_type: String,
        description: String,
        color: Option<String>,
    },
    /// Subagent completed.
    SubagentCompleted {
        agent_id: String,
        result: String,
        is_error: bool,
    },
    /// MCP server status changed.
    McpStatus {
        server_name: String,
        connected: bool,
        tool_count: i32,
    },
    /// Error event.
    Error { message: String, retryable: bool },
    /// Plan mode changed.
    PlanModeChanged { entered: bool },
    /// Context compacted.
    ContextCompacted {
        removed_messages: i32,
        summary_tokens: i32,
    },
    /// Permission request (triggers overlay).
    PermissionRequest {
        request_id: String,
        tool_name: String,
        description: String,
        input_preview: String,
    },
    /// Diff stats loaded for a message (response to RequestDiffStats).
    /// TS: fileHistoryGetDiffStats() result in MessageSelector useEffect.
    DiffStatsLoaded {
        message_id: String,
        files_changed: i32,
        insertions: i64,
        deletions: i64,
        has_any_changes: bool,
    },
    /// Rewind completed — TUI should truncate messages and restore state.
    ///
    /// TS: rewindConversationTo() in REPL.tsx — truncates messages, resets
    /// conversation ID, restores permission mode. The TUI extracts
    /// permission_mode and input_text from its own message list (not from
    /// this notification) since the agent driver doesn't own the messages.
    RewindCompleted {
        /// UUID of the target user message. Empty = code-only rewind (no truncation).
        target_message_id: String,
        /// Number of files restored (0 if conversation-only).
        files_changed: i32,
    },
    /// Session ended.
    SessionEnded { reason: String },
}

/// Handle a CoreEvent from the agent loop.
///
/// Translates the event into zero or more internal `TuiNotification`s and
/// applies them. Returns `true` if any redraw is needed.
///
/// See `event-system-design.md` Section 1.4 for the 3-layer CoreEvent model.
pub fn handle_core_event(state: &mut AppState, event: CoreEvent) -> bool {
    let notifications = core_event_to_tui_notifications(event, state);
    let mut needs_redraw = false;
    for n in notifications {
        needs_redraw |= handle_tui_notification(state, n);
    }
    needs_redraw
}

/// Translate a CoreEvent into zero or more TuiNotifications.
///
/// Protocol events map to lifecycle notifications; Stream events map to
/// streaming display notifications; Tui events map to overlay/toast actions.
fn core_event_to_tui_notifications(event: CoreEvent, state: &AppState) -> Vec<TuiNotification> {
    match event {
        CoreEvent::Protocol(n) => translate_protocol(n, state),
        CoreEvent::Stream(s) => translate_stream(s),
        CoreEvent::Tui(t) => translate_tui_only(t),
    }
}

fn translate_protocol(notif: ServerNotification, _state: &AppState) -> Vec<TuiNotification> {
    match notif {
        ServerNotification::TurnStarted(p) => vec![TuiNotification::TurnStarted {
            turn_number: p.turn_number,
        }],
        ServerNotification::TurnCompleted(p) => vec![TuiNotification::TurnCompleted {
            usage: TokenUsage {
                input_tokens: p.usage.input_tokens,
                output_tokens: p.usage.output_tokens,
                cache_read_tokens: p.usage.cache_read_input_tokens,
                cache_creation_tokens: p.usage.cache_creation_input_tokens,
            },
        }],
        ServerNotification::TurnFailed(p) => vec![TuiNotification::TurnFailed { error: p.error }],
        ServerNotification::SubagentSpawned(p) => vec![TuiNotification::SubagentSpawned {
            agent_id: p.agent_id,
            agent_type: p.agent_type,
            description: p.description,
            color: p.color,
        }],
        ServerNotification::SubagentCompleted(p) => vec![TuiNotification::SubagentCompleted {
            agent_id: p.agent_id,
            result: p.result,
            is_error: p.is_error,
        }],
        ServerNotification::McpStartupStatus(p) => vec![TuiNotification::McpStatus {
            server_name: p.server,
            connected: p.status == "connected",
            tool_count: 0,
        }],
        ServerNotification::Error(p) => vec![TuiNotification::Error {
            message: p.message,
            retryable: p.retryable,
        }],
        ServerNotification::PlanModeChanged(p) => {
            vec![TuiNotification::PlanModeChanged { entered: p.entered }]
        }
        ServerNotification::ContextCompacted(p) => vec![TuiNotification::ContextCompacted {
            removed_messages: p.removed_messages,
            summary_tokens: p.summary_tokens,
        }],
        ServerNotification::SessionEnded(p) => {
            vec![TuiNotification::SessionEnded { reason: p.reason }]
        }
        // Events not currently surfaced in the TUI are dropped silently.
        _ => vec![],
    }
}

fn translate_stream(event: AgentStreamEvent) -> Vec<TuiNotification> {
    match event {
        AgentStreamEvent::TextDelta { delta, .. } => vec![TuiNotification::TextDelta { delta }],
        AgentStreamEvent::ThinkingDelta { delta, .. } => {
            vec![TuiNotification::ThinkingDelta { delta }]
        }
        AgentStreamEvent::ToolUseQueued {
            call_id,
            name,
            input,
        } => {
            let input_preview = input.to_string();
            vec![TuiNotification::ToolUseQueued {
                call_id,
                name,
                input_preview,
            }]
        }
        AgentStreamEvent::ToolUseStarted { .. } => vec![], // in-progress indicator not wired yet
        AgentStreamEvent::ToolUseCompleted {
            call_id,
            name: _,
            output,
            is_error,
        } => vec![TuiNotification::ToolUseCompleted {
            call_id,
            output,
            is_error,
        }],
        AgentStreamEvent::McpToolCallBegin { .. } | AgentStreamEvent::McpToolCallEnd { .. } => {
            vec![]
        }
    }
}

fn translate_tui_only(event: TuiOnlyEvent) -> Vec<TuiNotification> {
    match event {
        TuiOnlyEvent::ApprovalRequired {
            request_id,
            tool_name,
            description,
            input_preview,
        } => vec![TuiNotification::PermissionRequest {
            request_id,
            tool_name,
            description,
            input_preview,
        }],
        TuiOnlyEvent::DiffStatsReady {
            message_id,
            files_changed,
            insertions,
            deletions,
        } => vec![TuiNotification::DiffStatsLoaded {
            message_id,
            files_changed,
            insertions,
            deletions,
            has_any_changes: files_changed > 0,
        }],
        TuiOnlyEvent::RewindCompleted {
            target_message_id,
            files_changed,
        } => vec![TuiNotification::RewindCompleted {
            target_message_id,
            files_changed,
        }],
        // QuestionAsked, ToolCallDelta, ToolProgress: not yet wired into state
        _ => vec![],
    }
}

/// Handle an internal TUI notification, updating state accordingly.
///
/// Returns `true` if the state changed and a redraw is needed.
pub fn handle_tui_notification(state: &mut AppState, notification: TuiNotification) -> bool {
    match notification {
        TuiNotification::TurnStarted { turn_number } => {
            state.session.turn_count = turn_number;
            state.session.set_busy(true);
            state.ui.streaming = Some(crate::state::ui::StreamingState::new());
            true
        }
        TuiNotification::TurnCompleted { usage } => {
            state.session.set_busy(false);
            state.session.update_tokens(usage);
            // Commit streaming content to messages
            if let Some(streaming) = state.ui.streaming.take()
                && !streaming.content.is_empty()
            {
                state.session.add_message(ChatMessage::assistant_text(
                    format!("turn-{}", state.session.turn_count),
                    streaming.content,
                ));
            }
            // Clear completed tools
            state
                .session
                .tool_executions
                .retain(|t| t.status == crate::state::session::ToolStatus::Running);

            // Auto-restore on interrupt: if the turn was user-cancelled and
            // conditions are met, auto-rewind to last user message.
            // TS: REPL.tsx lines 3010-3021 (restoreMessageSyncRef on user-cancel)
            if state.session.was_interrupted
                && state.ui.input.is_empty()
                && state.ui.overlay.is_none()
                && let Some(idx) =
                    crate::update_rewind::find_last_user_message_index(&state.session.messages)
                && crate::update_rewind::messages_after_are_only_synthetic(
                    &state.session.messages,
                    idx,
                )
            {
                // Lossless: auto-restore input and truncate
                let input_text = state.session.messages[idx].text_content().to_string();
                let perm = state.session.messages[idx].permission_mode;
                state.session.messages.truncate(idx);
                if let Some(mode) = perm {
                    state.session.permission_mode = mode;
                }
                if !input_text.is_empty() {
                    state.ui.input.text = input_text;
                    state.ui.input.cursor = state.ui.input.text.chars().count() as i32;
                }
                state.ui.scroll_offset = 0;
                state.ui.user_scrolled = false;
            }
            state.session.was_interrupted = false;
            true
        }
        TuiNotification::TurnFailed { error } => {
            state.session.set_busy(false);
            state.ui.streaming = None;
            state.ui.add_toast(Toast::error(error));
            true
        }
        TuiNotification::TextDelta { delta } => {
            if let Some(ref mut streaming) = state.ui.streaming {
                streaming.append_text(&delta);
            }
            true
        }
        TuiNotification::ThinkingDelta { delta } => {
            if let Some(ref mut streaming) = state.ui.streaming {
                streaming.append_thinking(&delta);
            }
            true
        }
        TuiNotification::ToolUseQueued {
            call_id,
            name,
            input_preview: _,
        } => {
            state.session.start_tool(call_id, name);
            true
        }
        TuiNotification::ToolUseCompleted {
            call_id,
            output,
            is_error,
        } => {
            state.session.complete_tool(&call_id, is_error);
            // Add tool result to messages
            let tool_name_str = state
                .session
                .tool_executions
                .iter()
                .find(|t| t.call_id == call_id)
                .map(|t| t.name.clone())
                .unwrap_or_default();
            if is_error {
                state.session.add_message(ChatMessage::tool_error(
                    format!("tool-{call_id}"),
                    &tool_name_str,
                    output,
                ));
            } else {
                state.session.add_message(ChatMessage::tool_success(
                    format!("tool-{call_id}"),
                    &tool_name_str,
                    output,
                ));
            }
            true
        }
        TuiNotification::SubagentSpawned {
            agent_id,
            agent_type,
            description,
            color,
        } => {
            state.session.subagents.push(SubagentInstance {
                agent_id,
                agent_type,
                description,
                status: SubagentStatus::Running,
                color,
            });
            true
        }
        TuiNotification::SubagentCompleted {
            agent_id,
            result: _,
            is_error,
        } => {
            if let Some(agent) = state
                .session
                .subagents
                .iter_mut()
                .find(|a| a.agent_id == agent_id)
            {
                agent.status = if is_error {
                    SubagentStatus::Failed
                } else {
                    SubagentStatus::Completed
                };
            }
            true
        }
        TuiNotification::McpStatus {
            server_name,
            connected,
            tool_count,
        } => {
            if let Some(server) = state
                .session
                .mcp_servers
                .iter_mut()
                .find(|s| s.name == server_name)
            {
                server.connected = connected;
                server.tool_count = tool_count;
            } else {
                state.session.mcp_servers.push(McpServerStatus {
                    name: server_name,
                    connected,
                    tool_count,
                });
            }
            true
        }
        TuiNotification::Error {
            message,
            retryable: _,
        } => {
            state.ui.add_toast(Toast::error(message));
            true
        }
        TuiNotification::PlanModeChanged { entered } => {
            state.session.plan_mode = entered;
            true
        }
        TuiNotification::ContextCompacted {
            removed_messages,
            summary_tokens: _,
        } => {
            state.session.is_compacting = false;
            state.ui.add_toast(Toast::info(format!(
                "Compacted {removed_messages} messages"
            )));
            true
        }
        TuiNotification::PermissionRequest {
            request_id,
            tool_name,
            description,
            input_preview,
        } => {
            state.ui.set_overlay(crate::state::Overlay::Permission(
                crate::state::PermissionOverlay {
                    request_id,
                    tool_name,
                    description,
                    detail: crate::state::ui::PermissionDetail::Generic { input_preview },
                    risk_level: None,
                    show_always_allow: true,
                    classifier_checking: false,
                    classifier_auto_approved: None,
                },
            ));
            true
        }
        TuiNotification::DiffStatsLoaded {
            message_id: stats_message_id,
            files_changed: diff_files,
            insertions,
            deletions,
            has_any_changes,
        } => {
            // Update the active rewind overlay with loaded diff stats.
            // Only apply if the stats are for the currently selected message
            // (guards against race: user navigated while stats were loading).
            // TS: MessageSelector useEffect sets diffStatsForRestore with cancellation.
            if let Some(crate::state::Overlay::Rewind(ref mut r)) = state.ui.overlay {
                let selected_id = r
                    .messages
                    .get(r.selected as usize)
                    .map(|m| m.message_id.as_str());
                if selected_id == Some(&stats_message_id) {
                    r.has_file_changes = has_any_changes;
                    r.diff_stats = Some(crate::state::DiffStatsPreview {
                        files_changed: diff_files,
                        insertions,
                        deletions,
                    });
                    // Rebuild restore options with accurate file change info
                    r.available_options = crate::state::rewind::build_restore_options(
                        r.file_history_enabled,
                        has_any_changes,
                    );
                }
            }
            true
        }
        TuiNotification::RewindCompleted {
            target_message_id,
            files_changed,
            ..
        } => {
            // Extract permission_mode and input_text from the target message
            // BEFORE truncating. TS: rewindConversationTo() reads from the
            // message itself, then textForResubmit() extracts input text.
            let mut restored_permission_mode = None;
            let mut restored_input_text = None;

            if !target_message_id.is_empty()
                && let Some(target_msg) = state
                    .session
                    .messages
                    .iter()
                    .find(|m| m.id == target_message_id)
            {
                // TS: message.permissionMode
                restored_permission_mode = target_msg.permission_mode;
                // TS: textForResubmit(message) — extract user input for
                // re-submission after rewind.
                restored_input_text =
                    Some(target_msg.text_content().to_string()).filter(|s| !s.is_empty());
            }

            // Truncate messages to the target message.
            // TS: setMessages(prev.slice(0, messageIndex))
            if !target_message_id.is_empty()
                && let Some(idx) = state
                    .session
                    .messages
                    .iter()
                    .position(|m| m.id == target_message_id)
            {
                state.session.messages.truncate(idx);
            }

            // Restore permission mode.
            // TS: toolPermissionContext.mode = message.permissionMode
            if let Some(mode) = restored_permission_mode {
                state.session.permission_mode = mode;
            }

            // Repopulate input text for re-submission.
            // TS: textForResubmit(message) → setInputValue(text)
            if let Some(text) = restored_input_text {
                state.ui.input.text = text;
                state.ui.input.cursor = state.ui.input.text.chars().count() as i32;
            }

            // Reset scroll state
            state.ui.scroll_offset = 0;
            state.ui.user_scrolled = false;

            // Dismiss any active overlay (rewind confirming state)
            state.ui.dismiss_overlay();

            let msg = if files_changed > 0 {
                format!("Rewound to checkpoint. {files_changed} files restored.")
            } else {
                "Conversation rewound to checkpoint.".to_string()
            };
            state.ui.add_toast(Toast::success(msg));
            true
        }
        TuiNotification::SessionEnded { reason: _ } => {
            state.quit();
            true
        }
    }
}

#[cfg(test)]
#[path = "server_notification_handler.test.rs"]
mod tests;
