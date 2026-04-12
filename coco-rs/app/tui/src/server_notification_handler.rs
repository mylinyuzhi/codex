//! Server notification handler — processes protocol events from the agent loop.
//!
//! Maps agent lifecycle events (turn started/completed, tool use, subagent spawn)
//! to SessionState mutations.

use crate::state::AppState;
use crate::state::session::ChatMessage;
use crate::state::session::McpServerStatus;
use crate::state::session::SubagentInstance;
use crate::state::session::SubagentStatus;
use crate::state::session::TokenUsage;
use crate::state::ui::Toast;

/// A protocol notification from the agent loop.
///
/// These map to the `ServerNotification` variants from `event-system-design.md`.
/// Simplified subset for initial implementation.
#[derive(Debug, Clone)]
pub enum ServerNotification {
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

/// Handle a server notification, updating state accordingly.
///
/// Returns `true` if the state changed and a redraw is needed.
pub fn handle_server_notification(state: &mut AppState, notification: ServerNotification) -> bool {
    match notification {
        ServerNotification::TurnStarted { turn_number } => {
            state.session.turn_count = turn_number;
            state.session.set_busy(true);
            state.ui.streaming = Some(crate::state::ui::StreamingState::new());
            true
        }
        ServerNotification::TurnCompleted { usage } => {
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
            {
                if let Some(idx) =
                    crate::update_rewind::find_last_user_message_index(&state.session.messages)
                {
                    if crate::update_rewind::messages_after_are_only_synthetic(
                        &state.session.messages,
                        idx,
                    ) {
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
                }
            }
            state.session.was_interrupted = false;
            true
        }
        ServerNotification::TurnFailed { error } => {
            state.session.set_busy(false);
            state.ui.streaming = None;
            state.ui.add_toast(Toast::error(error));
            true
        }
        ServerNotification::TextDelta { delta } => {
            if let Some(ref mut streaming) = state.ui.streaming {
                streaming.append_text(&delta);
            }
            true
        }
        ServerNotification::ThinkingDelta { delta } => {
            if let Some(ref mut streaming) = state.ui.streaming {
                streaming.append_thinking(&delta);
            }
            true
        }
        ServerNotification::ToolUseQueued {
            call_id,
            name,
            input_preview: _,
        } => {
            state.session.start_tool(call_id, name);
            true
        }
        ServerNotification::ToolUseCompleted {
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
        ServerNotification::SubagentSpawned {
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
        ServerNotification::SubagentCompleted {
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
        ServerNotification::McpStatus {
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
        ServerNotification::Error {
            message,
            retryable: _,
        } => {
            state.ui.add_toast(Toast::error(message));
            true
        }
        ServerNotification::PlanModeChanged { entered } => {
            state.session.plan_mode = entered;
            true
        }
        ServerNotification::ContextCompacted {
            removed_messages,
            summary_tokens: _,
        } => {
            state.session.is_compacting = false;
            state.ui.add_toast(Toast::info(format!(
                "Compacted {removed_messages} messages"
            )));
            true
        }
        ServerNotification::PermissionRequest {
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
        ServerNotification::DiffStatsLoaded {
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
        ServerNotification::RewindCompleted {
            target_message_id,
            files_changed,
            ..
        } => {
            // Extract permission_mode and input_text from the target message
            // BEFORE truncating. TS: rewindConversationTo() reads from the
            // message itself, then textForResubmit() extracts input text.
            let mut restored_permission_mode = None;
            let mut restored_input_text = None;

            if !target_message_id.is_empty() {
                if let Some(target_msg) = state
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
            }

            // Truncate messages to the target message.
            // TS: setMessages(prev.slice(0, messageIndex))
            if !target_message_id.is_empty() {
                if let Some(idx) = state
                    .session
                    .messages
                    .iter()
                    .position(|m| m.id == target_message_id)
                {
                    state.session.messages.truncate(idx);
                }
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
        ServerNotification::SessionEnded { reason: _ } => {
            state.quit();
            true
        }
    }
}

#[cfg(test)]
#[path = "server_notification_handler.test.rs"]
mod tests;
