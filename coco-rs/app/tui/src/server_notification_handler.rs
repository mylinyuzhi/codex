//! Server notification handler — processes protocol events from the agent loop.
//!
//! Architecture (post WS-2 refactor): this module receives `CoreEvent` from
//! the agent loop and dispatches directly to three exhaustive handlers:
//!
//! - `handle_protocol(state, ServerNotification)` — all protocol variants (exhaustive)
//! - `handle_stream(state, AgentStreamEvent)` — all stream variants (exhaustive)
//! - `handle_tui_only(state, TuiOnlyEvent)` — TUI-exclusive variants
//!
//! The old `TuiNotification` bridge type has been deleted. See
//! `event-system-design.md` §1.7-1.8 and plan file WS-2 for rationale:
//! - 75% of variants were trivial pass-throughs with no real adaptation
//! - Scaling to 57 variants would create a 1:1 copy, tripling maintenance
//! - The TUI is not classical TEA; `TuiNotification` was a private
//!   intermediate for one of two orthogonal dispatch paths
//! - TS has no equivalent (direct dispatch via handleMessageFromStream)
//!
//! Complex handler logic (TurnCompleted auto-restore, RewindCompleted
//! truncation) is extracted into named private functions for readability.

use coco_types::AgentStreamEvent;
use coco_types::CoreEvent;
use coco_types::PermissionMode;
use coco_types::ServerNotification;
use coco_types::TuiOnlyEvent;

use crate::state::AppState;
use crate::state::session::ChatMessage;
use crate::state::session::HookEntry;
use crate::state::session::HookEntryStatus;
use crate::state::session::McpServerStatus;
use crate::state::session::RateLimitInfo;
use crate::state::session::SubagentInstance;
use crate::state::session::SubagentStatus;
use crate::state::session::TaskEntry;
use crate::state::session::TaskEntryStatus;
use crate::state::session::TokenUsage;
use crate::state::ui::Toast;

/// Handle a CoreEvent from the agent loop.
///
/// Dispatches to exhaustive sub-handlers for each `CoreEvent` layer.
/// Returns `true` if any state changed and a redraw is needed.
pub fn handle_core_event(state: &mut AppState, event: CoreEvent) -> bool {
    match event {
        CoreEvent::Protocol(notif) => handle_protocol(state, notif),
        CoreEvent::Stream(stream_evt) => handle_stream(state, stream_evt),
        CoreEvent::Tui(tui_evt) => handle_tui_only(state, tui_evt),
    }
}

// ---------------------------------------------------------------------------
// Protocol layer — ServerNotification (57 variants)
// ---------------------------------------------------------------------------

fn handle_protocol(state: &mut AppState, notif: ServerNotification) -> bool {
    match notif {
        // === Session lifecycle ===
        ServerNotification::SessionStarted(p) => {
            state.session.session_id = Some(p.session_id);
            state.session.model = p.model;
            state.session.working_dir = Some(p.cwd);
            true
        }
        ServerNotification::SessionResult(p) => {
            state.session.estimated_cost_cents = (p.total_cost_usd * 100.0) as i32;
            true
        }
        ServerNotification::SessionEnded(p) => {
            let _ = p.reason;
            state.quit();
            true
        }

        // === Turn lifecycle ===
        ServerNotification::TurnStarted(p) => {
            state.session.turn_count = p.turn_number;
            state.session.set_busy(true);
            state.session.stream_stall = false;
            state.ui.streaming = Some(crate::state::ui::StreamingState::new());
            true
        }
        ServerNotification::TurnCompleted(p) => on_turn_completed(state, p),
        ServerNotification::TurnFailed(p) => {
            state.session.set_busy(false);
            state.ui.streaming = None;
            state.ui.add_toast(Toast::error(p.error));
            true
        }
        ServerNotification::TurnInterrupted(_) => {
            state.session.set_busy(false);
            state.session.was_interrupted = true;
            state
                .ui
                .add_toast(Toast::warning("Turn interrupted".to_string()));
            true
        }
        ServerNotification::MaxTurnsReached { max_turns } => {
            let msg = match max_turns {
                Some(n) => format!("Max turns reached ({n})"),
                None => "Max turns reached".into(),
            };
            state.ui.add_toast(Toast::warning(msg));
            true
        }

        // === Item lifecycle (SDK-path only; TUI uses Stream layer) ===
        // The TUI gets real-time display from AgentStreamEvent (TextDelta,
        // ToolUseQueued, etc.) in handle_stream(). The Item* protocol events
        // are produced by StreamAccumulator for SDK consumers and are
        // intentionally no-ops in the TUI.
        ServerNotification::ItemStarted { .. }
        | ServerNotification::ItemUpdated { .. }
        | ServerNotification::ItemCompleted { .. } => false,

        // === Content deltas (SDK path — TUI uses Stream layer) ===
        ServerNotification::AgentMessageDelta(_) | ServerNotification::ReasoningDelta(_) => false,

        // === Subagent ===
        ServerNotification::SubagentSpawned(p) => {
            state.session.subagents.push(SubagentInstance {
                agent_id: p.agent_id,
                agent_type: p.agent_type,
                description: p.description,
                status: SubagentStatus::Running,
                color: p.color,
            });
            true
        }
        ServerNotification::SubagentCompleted(p) => {
            if let Some(agent) = state
                .session
                .subagents
                .iter_mut()
                .find(|a| a.agent_id == p.agent_id)
            {
                agent.status = if p.is_error {
                    SubagentStatus::Failed
                } else {
                    SubagentStatus::Completed
                };
            }
            true
        }
        ServerNotification::SubagentBackgrounded(p) => {
            if let Some(agent) = state
                .session
                .subagents
                .iter_mut()
                .find(|a| a.agent_id == p.agent_id)
            {
                agent.status = SubagentStatus::Backgrounded;
            }
            true
        }
        ServerNotification::SubagentProgress(p) => {
            if let Some(msg) = &p.summary {
                state
                    .ui
                    .add_toast(Toast::info(format!("Agent {}: {msg}", p.agent_id)));
            }
            true
        }

        // === MCP ===
        ServerNotification::McpStartupStatus(p) => {
            let connected = matches!(p.status, coco_types::McpConnectionStatus::Connected);
            if let Some(server) = state
                .session
                .mcp_servers
                .iter_mut()
                .find(|s| s.name == p.server)
            {
                server.connected = connected;
            } else {
                state.session.mcp_servers.push(McpServerStatus {
                    name: p.server,
                    connected,
                    tool_count: 0,
                });
            }
            true
        }
        ServerNotification::McpStartupComplete(p) => {
            if !p.failed.is_empty() {
                state.ui.add_toast(Toast::warning(format!(
                    "MCP: {} failed to start",
                    p.failed.len()
                )));
            }
            true
        }

        // === Context ===
        ServerNotification::ContextCompacted(p) => {
            state.session.is_compacting = false;
            state.ui.add_toast(Toast::info(format!(
                "Compacted {} messages",
                p.removed_messages
            )));
            true
        }
        ServerNotification::ContextUsageWarning(p) => {
            state.session.context_usage_percent = Some(p.percent_left);
            if p.percent_left < 10.0 {
                state.ui.add_toast(Toast::warning(format!(
                    "Context {:.0}% remaining",
                    p.percent_left
                )));
            }
            true
        }
        ServerNotification::CompactionStarted => {
            state.session.is_compacting = true;
            true
        }
        ServerNotification::CompactionFailed(p) => {
            state.session.is_compacting = false;
            state
                .ui
                .add_toast(Toast::error(format!("Compaction failed: {}", p.error)));
            true
        }
        ServerNotification::ContextCleared(_) => {
            state
                .ui
                .add_toast(Toast::info("Context cleared".to_string()));
            true
        }

        // === Task ===
        ServerNotification::TaskStarted(p) => {
            state.session.active_tasks.push(TaskEntry {
                task_id: p.task_id,
                description: p.description,
                status: TaskEntryStatus::Running,
            });
            true
        }
        ServerNotification::TaskCompleted(p) => {
            let status = match p.status {
                coco_types::TaskCompletionStatus::Completed => TaskEntryStatus::Completed,
                coco_types::TaskCompletionStatus::Failed => TaskEntryStatus::Failed,
                coco_types::TaskCompletionStatus::Stopped => TaskEntryStatus::Stopped,
            };
            if let Some(task) = state
                .session
                .active_tasks
                .iter_mut()
                .find(|t| t.task_id == p.task_id)
            {
                task.status = status;
            }
            true
        }
        ServerNotification::TaskProgress(p) => {
            if let Some(task) = state
                .session
                .active_tasks
                .iter_mut()
                .find(|t| t.task_id == p.task_id)
            {
                task.description = p.description;
            }
            true
        }
        ServerNotification::AgentsKilled(p) => {
            state
                .ui
                .add_toast(Toast::info(format!("{} agents killed", p.count)));
            true
        }

        // === Model ===
        ServerNotification::ModelFallbackStarted(p) => {
            state.session.model_fallback_banner =
                Some(format!("{} → {}", p.from_model, p.to_model));
            state.session.model = p.to_model;
            state.ui.add_toast(Toast::warning(format!(
                "Fallback: {}",
                state.session.model_fallback_banner.as_deref().unwrap_or("")
            )));
            true
        }
        ServerNotification::ModelFallbackCompleted => {
            state.session.model_fallback_banner = None;
            true
        }
        ServerNotification::FastModeChanged { active } => {
            let msg = if active {
                "Fast mode on"
            } else {
                "Fast mode off"
            };
            state.ui.add_toast(Toast::info(msg.to_string()));
            true
        }

        // === Permission ===
        ServerNotification::PermissionModeChanged(p) => {
            if let Ok(mode) =
                serde_json::from_value::<PermissionMode>(serde_json::Value::String(p.mode))
            {
                state.session.permission_mode = mode;
            }
            true
        }

        // === Prompt ===
        ServerNotification::PromptSuggestion { suggestions } => {
            state.session.prompt_suggestions = suggestions;
            true
        }

        // === System ===
        ServerNotification::Error(p) => {
            state.ui.add_toast(Toast::error(p.message));
            true
        }
        ServerNotification::RateLimit(p) => {
            state.session.rate_limit_info = Some(RateLimitInfo {
                remaining: p.remaining,
                reset_at: p.reset_at,
                provider: p.provider,
            });
            if p.remaining == Some(0) {
                state
                    .ui
                    .add_toast(Toast::warning("Rate limited".to_string()));
            }
            true
        }
        ServerNotification::KeepAlive { .. } => false,

        // === IDE (state stored for future ide_panel widget) ===
        ServerNotification::IdeSelectionChanged(_) => false,
        ServerNotification::IdeDiagnosticsUpdated(_) => false,

        // === Plan ===
        ServerNotification::PlanModeChanged(p) => {
            state.session.plan_mode = p.entered;
            true
        }

        // === Queue ===
        ServerNotification::QueueStateChanged { queued } => {
            state
                .session
                .queued_commands
                .truncate(queued.max(0) as usize);
            true
        }
        ServerNotification::CommandQueued { id, preview } => {
            let _ = id;
            state.session.queued_commands.push(preview);
            true
        }
        ServerNotification::CommandDequeued { id } => {
            let _ = id;
            if !state.session.queued_commands.is_empty() {
                state.session.queued_commands.remove(0);
            }
            true
        }

        // === Rewind ===
        ServerNotification::RewindCompleted(p) => {
            // Rewind from protocol layer (vs TuiOnlyEvent::RewindCompleted
            // which carries the target_message_id). Protocol-level rewind
            // carries restored_files count; TUI toast only.
            let msg = if p.restored_files > 0 {
                format!("Rewound. {} files restored.", p.restored_files)
            } else {
                "Conversation rewound.".to_string()
            };
            state.ui.add_toast(Toast::success(msg));
            true
        }
        ServerNotification::RewindFailed { error } => {
            state
                .ui
                .add_toast(Toast::error(format!("Rewind failed: {error}")));
            true
        }

        // === Cost ===
        ServerNotification::CostWarning(p) => {
            state.ui.add_toast(Toast::warning(format!(
                "Cost: {}c / {}c threshold",
                p.current_cost_cents, p.threshold_cents
            )));
            true
        }

        // === Sandbox ===
        ServerNotification::SandboxStateChanged(p) => {
            state.session.sandbox_active = p.active;
            true
        }
        ServerNotification::SandboxViolationsDetected { count } => {
            state
                .ui
                .add_toast(Toast::error(format!("{count} sandbox violations")));
            true
        }

        // === Agent ===
        ServerNotification::AgentsRegistered { agents } => {
            if !agents.is_empty() {
                state
                    .ui
                    .add_toast(Toast::info(format!("{} agents registered", agents.len())));
            }
            true
        }

        // === Hook ===
        ServerNotification::HookExecuted(_) => false,
        ServerNotification::HookStarted(p) => {
            state.session.active_hooks.push(HookEntry {
                hook_id: p.hook_id,
                hook_name: p.hook_name,
                status: HookEntryStatus::Running,
                output: None,
            });
            true
        }
        ServerNotification::HookProgress(p) => {
            if let Some(hook) = state
                .session
                .active_hooks
                .iter_mut()
                .find(|h| h.hook_id == p.hook_id)
            {
                if !p.stdout.is_empty() {
                    hook.output = Some(p.stdout);
                }
            }
            true
        }
        ServerNotification::HookResponse(p) => {
            if let Some(hook) = state
                .session
                .active_hooks
                .iter_mut()
                .find(|h| h.hook_id == p.hook_id)
            {
                hook.status = match p.outcome {
                    coco_types::HookOutcomeStatus::Success => HookEntryStatus::Completed,
                    _ => HookEntryStatus::Failed,
                };
                hook.output = Some(p.output);
            }
            true
        }

        // === Worktree ===
        ServerNotification::WorktreeEntered(p) => {
            state.session.worktree_path = Some(p.worktree_path);
            true
        }
        ServerNotification::WorktreeExited(_) => {
            state.session.worktree_path = None;
            true
        }

        // === Summarize ===
        ServerNotification::SummarizeCompleted(_) => {
            state
                .ui
                .add_toast(Toast::info("Summarization complete".to_string()));
            true
        }
        ServerNotification::SummarizeFailed { error } => {
            state
                .ui
                .add_toast(Toast::error(format!("Summarize failed: {error}")));
            true
        }

        // === Stream health ===
        ServerNotification::StreamStallDetected { .. } => {
            state.session.stream_stall = true;
            state
                .ui
                .add_toast(Toast::warning("Stream stall detected".to_string()));
            true
        }
        ServerNotification::StreamWatchdogWarning { elapsed_secs } => {
            state.ui.add_toast(Toast::warning(format!(
                "Stream watchdog: {elapsed_secs:.0}s without data"
            )));
            true
        }
        ServerNotification::StreamRequestEnd { .. } => false,

        // === Session state ===
        ServerNotification::SessionStateChanged { state: new_state } => {
            state.session.session_state = new_state;
            true
        }

        // === TS P2 additions ===
        ServerNotification::LocalCommandOutput(p) => {
            const MAX_LOCAL_OUTPUT: usize = 50;
            state
                .session
                .local_command_output
                .push(p.content.to_string());
            if state.session.local_command_output.len() > MAX_LOCAL_OUTPUT {
                state.session.local_command_output.remove(0);
            }
            true
        }
        ServerNotification::FilesPersisted(p) => {
            let count = p.files.len();
            let failed = p.failed.len();
            let msg = if failed > 0 {
                format!("{count} files persisted, {failed} failed")
            } else {
                format!("{count} files persisted")
            };
            state.ui.add_toast(Toast::info(msg));
            true
        }
        ServerNotification::ElicitationComplete(_) => {
            state.ui.dismiss_overlay();
            true
        }
        ServerNotification::ToolUseSummary(p) => {
            state.session.add_message(ChatMessage::system_text(
                format!(
                    "summary-{}",
                    p.preceding_tool_use_ids.first().unwrap_or(&String::new())
                ),
                p.summary,
            ));
            true
        }
        ServerNotification::ToolProgress(p) => {
            if let Some(tool) = state.session.tool_executions.iter_mut().find(|t| {
                t.call_id == p.tool_use_id || Some(&t.call_id) == p.parent_tool_use_id.as_ref()
            }) {
                tool.description = Some(format!("{}s", p.elapsed_time_seconds));
            }
            true
        }
    }
}

// ---------------------------------------------------------------------------
// Stream layer — AgentStreamEvent (7 variants)
// ---------------------------------------------------------------------------

fn handle_stream(state: &mut AppState, event: AgentStreamEvent) -> bool {
    match event {
        AgentStreamEvent::TextDelta { delta, .. } => {
            if let Some(ref mut streaming) = state.ui.streaming {
                streaming.append_text(&delta);
            }
            true
        }
        AgentStreamEvent::ThinkingDelta { delta, .. } => {
            if let Some(ref mut streaming) = state.ui.streaming {
                streaming.append_thinking(&delta);
            }
            true
        }
        AgentStreamEvent::ToolUseQueued { call_id, name, .. } => {
            state.session.start_tool(call_id, name);
            true
        }
        AgentStreamEvent::ToolUseStarted { .. } => false,
        AgentStreamEvent::ToolUseCompleted {
            call_id,
            name: _,
            output,
            is_error,
        } => {
            state.session.complete_tool(&call_id, is_error);
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
        AgentStreamEvent::McpToolCallBegin { .. } | AgentStreamEvent::McpToolCallEnd { .. } => {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// TUI-only layer — TuiOnlyEvent (~20 variants)
// ---------------------------------------------------------------------------

fn handle_tui_only(state: &mut AppState, event: TuiOnlyEvent) -> bool {
    match event {
        TuiOnlyEvent::ApprovalRequired {
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
        TuiOnlyEvent::DiffStatsReady {
            message_id,
            files_changed,
            insertions,
            deletions,
        } => on_diff_stats_loaded(state, message_id, files_changed, insertions, deletions),
        TuiOnlyEvent::RewindCompleted {
            target_message_id,
            files_changed,
        } => on_rewind_completed(state, target_message_id, files_changed),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Extracted private handler functions for complex logic
// ---------------------------------------------------------------------------

fn on_turn_completed(state: &mut AppState, p: coco_types::TurnCompletedParams) -> bool {
    state.session.set_busy(false);
    state.session.update_tokens(TokenUsage {
        input_tokens: p.usage.input_tokens,
        output_tokens: p.usage.output_tokens,
        cache_read_tokens: p.usage.cache_read_input_tokens,
        cache_creation_tokens: p.usage.cache_creation_input_tokens,
    });
    if let Some(streaming) = state.ui.streaming.take()
        && !streaming.content.is_empty()
    {
        state.session.add_message(ChatMessage::assistant_text(
            format!("turn-{}", state.session.turn_count),
            streaming.content,
        ));
    }
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
        && crate::update_rewind::messages_after_are_only_synthetic(&state.session.messages, idx)
    {
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

fn on_diff_stats_loaded(
    state: &mut AppState,
    stats_message_id: String,
    diff_files: i32,
    insertions: i64,
    deletions: i64,
) -> bool {
    let has_any_changes = diff_files > 0;
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
            r.available_options = crate::state::rewind::build_restore_options(
                r.file_history_enabled,
                has_any_changes,
            );
        }
    }
    true
}

fn on_rewind_completed(
    state: &mut AppState,
    target_message_id: String,
    files_changed: i32,
) -> bool {
    let mut restored_permission_mode = None;
    let mut restored_input_text = None;

    if !target_message_id.is_empty()
        && let Some(target_msg) = state
            .session
            .messages
            .iter()
            .find(|m| m.id == target_message_id)
    {
        restored_permission_mode = target_msg.permission_mode;
        restored_input_text = Some(target_msg.text_content().to_string()).filter(|s| !s.is_empty());
    }

    if !target_message_id.is_empty()
        && let Some(idx) = state
            .session
            .messages
            .iter()
            .position(|m| m.id == target_message_id)
    {
        state.session.messages.truncate(idx);
    }

    if let Some(mode) = restored_permission_mode {
        state.session.permission_mode = mode;
    }

    if let Some(text) = restored_input_text {
        state.ui.input.text = text;
        state.ui.input.cursor = state.ui.input.text.chars().count() as i32;
    }

    state.ui.scroll_offset = 0;
    state.ui.user_scrolled = false;
    state.ui.dismiss_overlay();

    let msg = if files_changed > 0 {
        format!("Rewound to checkpoint. {files_changed} files restored.")
    } else {
        "Conversation rewound to checkpoint.".to_string()
    };
    state.ui.add_toast(Toast::success(msg));
    true
}

#[cfg(test)]
#[path = "server_notification_handler.test.rs"]
mod tests;
