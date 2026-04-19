//! Protocol-layer handler.
//!
//! Handles all 65 [`ServerNotification`] variants. The TUI matches
//! exhaustively — adding a new variant in `coco-types` fails compilation
//! here until a TUI behavior is chosen (even if that behavior is an
//! explicit `false` no-op with a comment).
//!
//! Item lifecycle (`ItemStarted`, `ItemUpdated`, `ItemCompleted`,
//! `AgentMessageDelta`, `ReasoningDelta`) are intentionally no-ops: they're
//! produced by `StreamAccumulator` for SDK consumers and never reach the
//! TUI channel in the current architecture. See `event-system-design.md`
//! §12 for the consumer routing matrix.

use coco_types::ServerNotification;

use crate::i18n::t;
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

pub(super) fn handle(state: &mut AppState, notif: ServerNotification) -> bool {
    match notif {
        // === Session lifecycle ===
        ServerNotification::SessionStarted(p) => {
            state.session.session_id = Some(p.session_id);
            state.session.model = p.model;
            state.session.working_dir = Some(p.cwd);
            // A new session invalidates any cached agent markdown — otherwise
            // /copy could surface text from a previous conversation.
            state.session.last_agent_markdown = None;
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
            // Keep the toast for session history / notification log, AND
            // raise a modal error dialog so users can't miss the failure
            // (PR-F1 P0). The toast auto-expires; the overlay blocks input
            // until dismissed.
            state.ui.add_toast(Toast::error(
                t!("toast.turn_failed_short", error = p.error.as_str()).to_string(),
            ));
            let body = crate::widgets::error_dialog::turn_failed_body(&p);
            state.ui.set_overlay(crate::state::Overlay::Error(body));
            true
        }
        ServerNotification::TurnInterrupted(_) => {
            state.session.set_busy(false);
            state.session.was_interrupted = true;
            state
                .ui
                .add_toast(Toast::warning(t!("toast.turn_interrupted").to_string()));
            true
        }
        ServerNotification::MaxTurnsReached { max_turns } => {
            let msg = match max_turns {
                Some(n) => t!("toast.max_turns_reached", n = n).to_string(),
                None => t!("toast.max_turns_unbounded").to_string(),
            };
            state.ui.add_toast(Toast::warning(msg.clone()));
            // Reaching the turn limit stops the loop; require explicit
            // acknowledgement before the user sends another prompt so
            // they notice it isn't silently continuing.
            let body = crate::widgets::error_dialog::format_error_body(&msg, Some("limit"), false);
            state.ui.set_overlay(crate::state::Overlay::Error(body));
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
                state.ui.add_toast(Toast::info(
                    t!(
                        "toast.agent_progress",
                        id = p.agent_id.as_str(),
                        msg = msg.as_str()
                    )
                    .to_string(),
                ));
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
                state.ui.add_toast(Toast::warning(
                    t!("toast.mcp_failed_to_start", count = p.failed.len()).to_string(),
                ));
            }
            true
        }

        // === Context ===
        ServerNotification::ContextCompacted(p) => {
            state.session.is_compacting = false;
            state.ui.add_toast(Toast::info(
                t!("toast.compacted_short", count = p.removed_messages).to_string(),
            ));
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
            // Compaction failures leave the session in a compromised state
            // (context still over budget); escalate past the toast.
            let msg = t!("toast.compaction_failed_short", error = p.error.as_str()).to_string();
            state.ui.add_toast(Toast::error(msg.clone()));
            let body =
                crate::widgets::error_dialog::format_error_body(&msg, Some("compaction"), false);
            state.ui.set_overlay(crate::state::Overlay::Error(body));
            true
        }
        ServerNotification::ContextCleared(_) => {
            state
                .ui
                .add_toast(Toast::info(t!("toast.context_cleared").to_string()));
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
            state.ui.add_toast(Toast::warning(
                t!(
                    "toast.fallback_short",
                    summary = state.session.model_fallback_banner.as_deref().unwrap_or("")
                )
                .to_string(),
            ));
            true
        }
        ServerNotification::ModelFallbackCompleted => {
            state.session.model_fallback_banner = None;
            true
        }
        ServerNotification::FastModeChanged { active } => {
            let msg = if active {
                t!("toast.fast_mode_on")
            } else {
                t!("toast.fast_mode_off")
            };
            state.ui.add_toast(Toast::info(msg.to_string()));
            true
        }

        // === Permission ===
        ServerNotification::PermissionModeChanged(p) => {
            state.session.permission_mode = p.mode;
            // Route the capability gate so the TUI overlay + Shift+Tab
            // cycle reflect the session's current authorization state.
            // Without this, `session.bypass_permissions_available`
            // stayed at its init-time default and no client could ever
            // cycle into BypassPermissions via the gate.
            state.session.bypass_permissions_available = p.bypass_available;
            true
        }

        // === Prompt ===
        ServerNotification::PromptSuggestion { suggestions } => {
            state.session.prompt_suggestions = suggestions;
            true
        }

        // === System ===
        ServerNotification::Error(p) => {
            // Retryable errors auto-recover; keep them as ephemeral toasts.
            // Non-retryable errors escalate to a modal overlay so the user
            // must acknowledge before continuing (PR-F1 P0).
            state.ui.add_toast(Toast::error(p.message.clone()));
            if !p.retryable {
                let body = crate::widgets::error_dialog::error_body(&p);
                state.ui.set_overlay(crate::state::Overlay::Error(body));
            }
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
                    .add_toast(Toast::warning(t!("toast.rate_limited").to_string()));
            }
            true
        }
        // No-op: heartbeat has no UI effect.
        ServerNotification::KeepAlive { .. } => false,

        // === IDE ===
        // Latest-wins replacement — widgets read the stored payload when rendering.
        ServerNotification::IdeSelectionChanged(p) => {
            state.session.ide_selection = Some(p);
            true
        }
        ServerNotification::IdeDiagnosticsUpdated(p) => {
            state.session.ide_diagnostics = Some(p);
            true
        }

        // === Plan ===
        // Plan-mode status is derived from `session.permission_mode`.
        // A dedicated PlanModeChanged notification flips the mode
        // directly; if the notification path is still carrying the
        // `entered` flag separately, translate to permission_mode here.
        ServerNotification::PlanModeChanged(p) => {
            state.session.permission_mode = if p.entered {
                coco_types::PermissionMode::Plan
            } else if state.session.permission_mode == coco_types::PermissionMode::Plan {
                coco_types::PermissionMode::Default
            } else {
                state.session.permission_mode
            };
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
            state.session.queued_commands.push_back(preview);
            true
        }
        ServerNotification::CommandDequeued { id } => {
            let _ = id;
            state.session.queued_commands.pop_front();
            true
        }

        // === Rewind ===
        ServerNotification::RewindCompleted(p) => {
            // Rewind from protocol layer (vs TuiOnlyEvent::RewindCompleted
            // which carries the target_message_id). Protocol-level rewind
            // carries restored_files count; TUI toast only.
            let msg = if p.restored_files > 0 {
                t!("toast.rewound_files", count = p.restored_files).to_string()
            } else {
                t!("toast.conversation_rewound").to_string()
            };
            state.ui.add_toast(Toast::success(msg));
            true
        }
        ServerNotification::RewindFailed { error } => {
            state.ui.add_toast(Toast::error(
                t!("toast.rewind_failed_short", error = error.as_str()).to_string(),
            ));
            true
        }

        // === Cost ===
        ServerNotification::CostWarning(p) => {
            // Budget breach is a decision point — route through the modal
            // `CostWarning` overlay (already defined) so users can stop
            // or continue explicitly. Keep the toast for the event log.
            let toast_msg = format!(
                "Cost: ${:.2} / ${:.2} threshold",
                p.current_cost_cents as f64 / 100.0,
                p.threshold_cents as f64 / 100.0
            );
            state.ui.add_toast(Toast::warning(toast_msg));
            state.ui.set_overlay(crate::state::Overlay::CostWarning(
                crate::state::CostWarningOverlay {
                    current_cost_cents: p.current_cost_cents,
                    threshold_cents: p.threshold_cents,
                },
            ));
            true
        }

        // === Sandbox ===
        ServerNotification::SandboxStateChanged(p) => {
            state.session.sandbox_active = p.active;
            true
        }
        ServerNotification::SandboxViolationsDetected { count } => {
            // Sandbox violations indicate the workload tried to do something
            // it shouldn't. Route through the modal so users review the
            // count before more violations stack up silently.
            state
                .ui
                .add_toast(Toast::error(format!("{count} sandbox violations")));
            let body = crate::widgets::error_dialog::format_error_body(
                &format!(
                    "Sandbox blocked {count} {}.",
                    if count == 1 {
                        "violation"
                    } else {
                        "violations"
                    }
                ),
                Some("sandbox"),
                false,
            );
            state.ui.set_overlay(crate::state::Overlay::Error(body));
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
                .add_toast(Toast::info(t!("toast.summarize_complete").to_string()));
            true
        }
        ServerNotification::SummarizeFailed { error } => {
            let msg = t!("toast.summarize_failed_short", error = error.as_str()).to_string();
            state.ui.add_toast(Toast::error(msg.clone()));
            let body =
                crate::widgets::error_dialog::format_error_body(&msg, Some("summarize"), false);
            state.ui.set_overlay(crate::state::Overlay::Error(body));
            true
        }

        // === Stream health ===
        ServerNotification::StreamStallDetected { .. } => {
            state.session.stream_stall = true;
            state.ui.add_toast(Toast::warning(
                t!("toast.stream_stall_detected").to_string(),
            ));
            true
        }
        ServerNotification::StreamWatchdogWarning { elapsed_secs } => {
            state.ui.add_toast(Toast::warning(
                t!("toast.stream_watchdog", secs = format!("{elapsed_secs:.0}")).to_string(),
            ));
            true
        }
        // No-op: usage tracked via TurnCompleted, not per-request.
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
                .push_back(p.content.to_string());
            while state.session.local_command_output.len() > MAX_LOCAL_OUTPUT {
                state.session.local_command_output.pop_front();
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

/// Handle `TurnCompleted`: finalize usage, flush streaming buffer into the
/// message list, prune completed tools, and auto-restore input on interrupt.
///
/// TS reference for auto-restore: `REPL.tsx` lines 3010-3021
/// (`restoreMessageSyncRef` on user-cancel).
fn on_turn_completed(state: &mut AppState, p: coco_types::TurnCompletedParams) -> bool {
    state.session.set_busy(false);
    state.session.update_tokens(TokenUsage {
        input_tokens: p.usage.input_tokens,
        output_tokens: p.usage.output_tokens,
        cache_read_tokens: p.usage.cache_read_input_tokens,
        cache_creation_tokens: p.usage.cache_creation_input_tokens,
    });
    // Emit a terminal notification when the user has switched away — they
    // typically want a ping when a long turn finishes in the background.
    // Skips when the terminal is focused to avoid pointless noise.
    if !state.ui.terminal_focused {
        crate::widgets::notification::notify(
            &t!("notification.app_name"),
            &t!("notification.turn_complete"),
        );
    }
    if let Some(streaming) = state.ui.streaming.take()
        && !streaming.content.is_empty()
    {
        // Cache raw markdown so /copy and Ctrl+O can reach it even after the
        // streaming buffer is consumed into the immutable message history.
        state.session.record_agent_markdown(&streaming.content);
        state.session.add_message(ChatMessage::assistant_text(
            format!("turn-{}", state.session.turn_count),
            streaming.content,
        ));
    }
    state.session.tool_executions.retain(|t| {
        matches!(
            t.status,
            crate::state::session::ToolStatus::Queued | crate::state::session::ToolStatus::Running
        )
    });

    // Auto-restore on interrupt: if the turn was user-cancelled and
    // conditions are met, auto-rewind to last user message.
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
