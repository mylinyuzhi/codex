//! Unified protocol handler for the TUI.
//!
//! Processes [`ServerNotification`]s and updates [`AppState`] -- the same
//! events that SDK clients receive. This is the PRIMARY handler for all
//! protocol-visible events. `tui_event_handler.rs` handles TUI-internal
//! events (overlays, tool tracking, queue sync) that have no
//! `ServerNotification` equivalent.

use cocode_app_server_protocol::ServerNotification;

use crate::i18n::t;
use crate::state::AppState;
use crate::state::ChatMessage;
use crate::state::InlineToolCall;
use crate::state::ToolStatus;

/// Handle a server notification from the unified protocol.
pub fn handle_server_notification(state: &mut AppState, notification: ServerNotification) {
    match notification {
        // ── Turn lifecycle ─────────────────────────────────────────────
        ServerNotification::TurnStarted(params) => {
            state.ui.start_streaming(params.turn_id);
            state.ui.clear_thinking_duration();
            state.session.reset_thinking_tokens();
            state.session.current_turn_number = Some(params.turn_number);
            if let Some(msg) = state.session.messages.last_mut()
                && msg.role == crate::state::MessageRole::User
                && msg.turn_number.is_none()
            {
                msg.turn_number = Some(params.turn_number);
            }
            state.ui.query_timing.start();
        }
        ServerNotification::TurnCompleted(params) => {
            if state.ui.query_timing.is_slow_query()
                && let Some(duration) = state.ui.query_timing.actual_duration()
            {
                state.ui.toast_info(
                    t!(
                        "toast.slow_query",
                        duration = format!("{:.1}", duration.as_secs_f64())
                    )
                    .to_string(),
                );
            }
            state.ui.query_timing.stop();
            state.ui.spinner_text = None;
            if state.ui.is_thinking() {
                state.ui.stop_thinking();
            }
            if let Some(mut streaming) = state.ui.streaming.take() {
                streaming.reveal_all();
                let mut message = ChatMessage::assistant(&params.turn_id, &streaming.content);
                if !streaming.thinking.is_empty() {
                    message.thinking = Some(streaming.thinking);
                }
                for tool_use in &streaming.tool_uses {
                    let session_tool = state
                        .session
                        .tool_executions
                        .iter()
                        .find(|t| t.name == tool_use.name);
                    message.tool_calls.push(InlineToolCall {
                        tool_name: tool_use.name.clone(),
                        status: session_tool.map_or(ToolStatus::Completed, |t| t.status),
                        description: extract_tool_description(&tool_use.accumulated_input),
                        elapsed: session_tool.and_then(|t| t.elapsed),
                        batch_id: session_tool.and_then(|t| t.batch_id.clone()),
                    });
                }
                for tool in &state.session.tool_executions {
                    if !message
                        .tool_calls
                        .iter()
                        .any(|tc| tc.tool_name == tool.name)
                    {
                        message.tool_calls.push(InlineToolCall {
                            tool_name: tool.name.clone(),
                            status: tool.status,
                            description: tool.progress.clone().unwrap_or_default(),
                            elapsed: tool.elapsed,
                            batch_id: tool.batch_id.clone(),
                        });
                    }
                }
                message.turn_number = state.session.current_turn_number;
                message.complete();
                state.session.add_message(message);
            }
            if let Some(reasoning_tokens) = params.usage.reasoning_tokens {
                state.session.add_thinking_tokens(reasoning_tokens as i32);
            }
            state.session.update_tokens_from_protocol(&params.usage);
        }
        ServerNotification::TurnFailed(params) => {
            state.ui.query_timing.stop();
            state.ui.spinner_text = None;
            state.ui.toast_error(params.error);
        }
        ServerNotification::TurnInterrupted(_) => {
            state.ui.stop_streaming();
            state.ui.query_timing.stop();
        }
        ServerNotification::MaxTurnsReached(_) => {
            state
                .ui
                .toast_info(t!("toast.max_turns_reached").to_string());
        }

        // ── Content streaming ──────────────────────────────────────────
        // Streaming deltas are handled by stream_event_handler via StreamEvent
        // because TUI has its own streaming buffer accumulation logic.
        ServerNotification::AgentMessageDelta(_) | ServerNotification::ReasoningDelta(_) => {}

        // ── Item lifecycle ─────────────────────────────────────────────
        // Tool items are handled by stream_event_handler via StreamEvent
        // because TUI tool tracking is call_id-based (not item_id-based).
        ServerNotification::ItemStarted(_)
        | ServerNotification::ItemUpdated(_)
        | ServerNotification::ItemCompleted(_) => {}

        // ── Subagent events ────────────────────────────────────────────
        ServerNotification::SubagentSpawned(params) => {
            state.session.start_subagent(
                params.agent_id,
                params.agent_type,
                params.description,
                params.color,
            );
        }
        ServerNotification::SubagentCompleted(params) => {
            let agent_name = state.session.subagent_type_name(&params.agent_id);
            let is_error = params.is_error;
            state
                .session
                .complete_subagent(&params.agent_id, params.result);
            state.session.cleanup_completed_subagents(5);
            if is_error {
                state
                    .ui
                    .toast_warning(t!("toast.subagent_failed", agent = agent_name).to_string());
            } else {
                state
                    .ui
                    .toast_success(t!("toast.subagent_completed", agent = agent_name).to_string());
            }
        }
        ServerNotification::SubagentBackgrounded(params) => {
            let agent_name = state.session.subagent_type_name(&params.agent_id);
            state
                .session
                .background_subagent(&params.agent_id, params.output_file.into());
            state
                .ui
                .toast_info(t!("toast.subagent_backgrounded", agent = agent_name).to_string());
        }

        // ── MCP events ─────────────────────────────────────────────────
        ServerNotification::McpStartupStatus(params) => match params.status.as_str() {
            "ready" => {
                state
                    .ui
                    .toast_success(t!("toast.mcp_ready", server = params.server).to_string());
            }
            "failed" => {
                state
                    .ui
                    .toast_error(t!("toast.mcp_failed", server = params.server).to_string());
            }
            _ => {
                tracing::debug!(server = %params.server, status = %params.status, "MCP startup");
            }
        },
        ServerNotification::McpStartupComplete(params) => {
            if !params.servers.is_empty() {
                let count = params.servers.len();
                state
                    .ui
                    .toast_success(t!("toast.mcp_connected", count = count).to_string());
            }
            for failure in params.failed {
                state.ui.toast_error(
                    t!(
                        "toast.mcp_error",
                        name = failure.name,
                        error = failure.error
                    )
                    .to_string(),
                );
            }
        }

        // ── Context management ─────────────────────────────────────────
        ServerNotification::ContextCompacted(params) => {
            let msg = t!(
                "toast.compacted",
                messages = params.removed_messages,
                tokens = params.summary_tokens
            )
            .to_string();
            state.ui.toast_success(msg);
            state.session.is_compacting = false;
            state.ui.spinner_text = None;
        }
        ServerNotification::ContextUsageWarning(params) => {
            state.session.context_window_used = params.estimated_tokens;
            state.session.context_window_total = params.warning_threshold;
            let format_tokens = |n: i32| -> String {
                if n >= 1_000_000 {
                    format!("{:.1}M", n as f64 / 1_000_000.0)
                } else if n >= 1_000 {
                    format!("{:.0}k", n as f64 / 1_000.0)
                } else {
                    n.to_string()
                }
            };
            let remain = format_tokens(
                params
                    .warning_threshold
                    .saturating_sub(params.estimated_tokens),
            );
            let total = format_tokens(params.warning_threshold);
            let percent = (params.percent_left * 100.0) as i32;
            state.ui.toast_warning(
                t!(
                    "toast.context_warning",
                    percent = percent,
                    remain = remain,
                    total = total
                )
                .to_string(),
            );
        }

        // ── Background tasks ───────────────────────────────────────────
        ServerNotification::TaskStarted(params) => {
            let task_type = match params.task_type.as_str() {
                "shell" => cocode_protocol::TaskType::Shell,
                "agent" => cocode_protocol::TaskType::Agent,
                "file_op" => cocode_protocol::TaskType::FileOp,
                other => cocode_protocol::TaskType::Other(other.to_string()),
            };
            state
                .session
                .start_background_task(params.task_id, task_type);
            state.ui.toast_info(
                t!(
                    "toast.background_task_started",
                    task_type = params.task_type
                )
                .to_string(),
            );
        }
        ServerNotification::TaskCompleted(params) => {
            state.session.complete_background_task(&params.task_id);
            state.session.cleanup_completed_background_tasks(5);
            state
                .ui
                .toast_success(t!("toast.background_task_completed").to_string());
        }

        // ── Model events ───────────────────────────────────────────────
        ServerNotification::ModelFallbackStarted(params) => {
            let msg = t!(
                "toast.model_fallback",
                from = params.from_model,
                to = params.to_model
            )
            .to_string();
            state.ui.toast_warning(msg);
            state.session.fallback_model = Some(params.to_model);
            tracing::info!(from = %params.from_model, reason = %params.reason, "Model fallback started");
        }

        // ── Permission events ──────────────────────────────────────────
        ServerNotification::PermissionModeChanged(params) => {
            if let Ok(mode) = params.mode.parse::<cocode_protocol::PermissionMode>() {
                state.session.permission_mode = mode;
                state.session.plan_mode = mode == cocode_protocol::PermissionMode::Plan;
            }
            // Latch bypass_available: once set to true, it stays true for the session.
            if params.bypass_available {
                state.session.bypass_available = true;
            }
        }

        // ── Session lifecycle ──────────────────────────────────────────
        ServerNotification::SessionStarted(_) | ServerNotification::SessionEnded(_) => {}
        ServerNotification::SessionResult(_) | ServerNotification::PromptSuggestion(_) => {}

        // ── System-level events ────────────────────────────────────────
        ServerNotification::Error(params) => {
            state.ui.query_timing.stop();
            state.ui.spinner_text = None;
            let error_text = params.message;
            state.ui.toast_error(error_text.clone());
            let mut msg = ChatMessage::system(
                format!(
                    "error-{}",
                    params
                        .category
                        .map_or("unknown".to_string(), |c| format!("{c:?}").to_lowercase())
                ),
                &error_text,
            );
            msg.turn_number = state.session.current_turn_number;
            state.session.add_message(msg);
        }
        ServerNotification::RateLimit(_) | ServerNotification::KeepAlive(_) => {}

        // ── IDE events ─────────────────────────────────────────────────
        ServerNotification::IdeSelectionChanged(_)
        | ServerNotification::IdeDiagnosticsUpdated(_) => {}

        // ── Plan mode ──────────────────────────────────────────────────
        ServerNotification::PlanModeChanged(params) => {
            state.session.plan_mode = params.entered;
            if params.entered {
                state.session.permission_mode = cocode_protocol::PermissionMode::Plan;
                state.session.plan_file = params.plan_file.map(std::path::PathBuf::from);
            } else {
                state.session.plan_file = None;
                if state.session.permission_mode == cocode_protocol::PermissionMode::Plan {
                    state.session.permission_mode = cocode_protocol::PermissionMode::Default;
                }
            }
        }

        // ── Queue ──────────────────────────────────────────────────────
        ServerNotification::QueueStateChanged(params) => {
            let local = state.session.queued_commands.len() as i32;
            if local != params.queued {
                tracing::warn!(
                    local,
                    core = params.queued,
                    "Queue count mismatch between TUI and core"
                );
            }
        }

        // ── Rewind ─────────────────────────────────────────────────────
        ServerNotification::RewindCompleted(params) => {
            if matches!(
                state.ui.overlay,
                Some(crate::state::Overlay::RewindSelector(_))
            ) {
                state.ui.clear_overlay();
            }
            state.ui.toast_success(
                t!(
                    "toast.rewind_success",
                    turn = params.rewound_turn,
                    files = params.restored_files
                )
                .to_string(),
            );
        }
        ServerNotification::RewindFailed(params) => {
            if matches!(
                state.ui.overlay,
                Some(crate::state::Overlay::RewindSelector(_))
            ) {
                state.ui.clear_overlay();
            }
            state
                .ui
                .toast_error(t!("toast.rewind_failed", error = params.error).to_string());
        }

        // ── Cost ───────────────────────────────────────────────────────
        ServerNotification::CostWarning(params) => {
            state.ui.set_overlay(crate::state::Overlay::CostWarning(
                crate::state::CostWarningOverlay::new(
                    params.current_cost_cents,
                    params.threshold_cents,
                    params.budget_cents,
                ),
            ));
        }

        // ── Sandbox ────────────────────────────────────────────────────
        ServerNotification::SandboxStateChanged(params) => {
            state.session.sandbox_active = params.active;
            if params.active {
                state.ui.toast_info(
                    t!("toast.sandbox_active", enforcement = params.enforcement).to_string(),
                );
            }
        }

        // ── Fast mode ──────────────────────────────────────────────────
        ServerNotification::FastModeChanged(params) => {
            state.session.fast_mode = params.active;
        }

        // ── Agents ─────────────────────────────────────────────────────
        ServerNotification::AgentsRegistered(_) => {
            // Agent autocomplete update handled in app.rs::handle_core_event
        }

        // ── Hook ───────────────────────────────────────────────────────
        ServerNotification::HookExecuted(params) => {
            tracing::debug!(
                hook_type = %params.hook_type,
                hook_name = %params.hook_name,
                "Hook executed"
            );
        }

        // ── Worktree events ───────────────────────────────────────────
        ServerNotification::WorktreeEntered(params) => {
            state
                .session
                .active_worktree_paths
                .push(crate::state::WorktreeInfo {
                    path: params.worktree_path,
                    branch: params.branch.clone(),
                });
            state
                .ui
                .toast_info(t!("toast.worktree_entered", branch = params.branch).to_string());
        }
        ServerNotification::WorktreeExited(params) => {
            state
                .session
                .active_worktree_paths
                .retain(|w| w.path != params.worktree_path);
            state.ui.toast_info(
                t!("toast.worktree_exited", action = params.action.as_str()).to_string(),
            );
        }

        // ── Summarize ──────────────────────────────────────────────────
        ServerNotification::SummarizeCompleted(params) => {
            if matches!(
                state.ui.overlay,
                Some(crate::state::Overlay::RewindSelector(_))
            ) {
                state.ui.clear_overlay();
            }
            state
                .ui
                .toast_success(t!("toast.summarize_success", turn = params.from_turn).to_string());
        }
        ServerNotification::SummarizeFailed(params) => {
            if matches!(
                state.ui.overlay,
                Some(crate::state::Overlay::RewindSelector(_))
            ) {
                state.ui.clear_overlay();
            }
            state
                .ui
                .toast_error(t!("toast.summarize_failed", error = params.error).to_string());
        }

        // ── Promoted events (formerly TUI-only Category C) ─────────────
        ServerNotification::TurnRetry(params) => {
            state.ui.toast_info(
                t!(
                    "toast.retry",
                    attempt = params.attempt,
                    max = params.max_attempts
                )
                .to_string(),
            );
        }
        ServerNotification::SubagentProgress(params) => {
            let progress = cocode_protocol::AgentProgress {
                message: params.message,
                current_step: params.current_step,
                total_steps: params.total_steps,
                summary: params.summary,
                activity: None,
            };
            state
                .session
                .update_subagent_progress(&params.agent_id, progress);
        }
        ServerNotification::CompactionStarted(_) => {
            state.ui.toast_info(t!("toast.compacting").to_string());
            state.session.is_compacting = true;
            state.ui.spinner_text = Some(t!("toast.compacting").to_string());
        }
        ServerNotification::CompactionFailed(params) => {
            state
                .ui
                .toast_error(t!("toast.compaction_failed", error = params.error).to_string());
            state.session.is_compacting = false;
            state.ui.spinner_text = None;
        }
        ServerNotification::ContextCleared(params) => {
            state.session.messages.clear();
            state.session.tool_executions.clear();
            state.session.subagents.clear();
            state.session.plan_mode = false;
            state.session.plan_file = None;
            if let Ok(mode) = params.new_mode.parse::<cocode_protocol::PermissionMode>() {
                state.session.permission_mode = mode;
            }
            state.ui.scroll_offset = 0;
            state.ui.user_scrolled = false;
        }
        ServerNotification::TaskProgress(params) => {
            if let Some(msg) = params.message {
                state
                    .session
                    .update_background_task_progress(&params.task_id, msg);
            }
        }
        ServerNotification::AgentsKilled(params) => {
            for id in &params.agent_ids {
                state.session.kill_subagent(id);
            }
            state
                .ui
                .toast_warning(t!("toast.agents_killed", count = params.count).to_string());
        }
        ServerNotification::ModelFallbackCompleted(_) => {
            state.session.fallback_model = None;
        }
        ServerNotification::CommandQueued(params) => {
            if !state
                .session
                .queued_commands
                .iter()
                .any(|c| c.id == params.id)
            {
                state
                    .session
                    .queued_commands
                    .push(cocode_protocol::UserQueuedCommand {
                        id: params.id.clone(),
                        prompt: params.preview,
                        queued_at: chrono::Utc::now().timestamp_millis(),
                        has_images: false,
                        command_mode: None,
                        origin: None,
                    });
            }
        }
        ServerNotification::CommandDequeued(params) => {
            state.session.queued_commands.retain(|c| c.id != params.id);
        }
        ServerNotification::SandboxViolationsDetected(params) => {
            state.session.sandbox_violation_count += params.count;
            // Flash violations in status bar; new violations extend the deadline.
            let deadline =
                std::time::Instant::now() + crate::constants::SANDBOX_VIOLATION_FLASH_DURATION;
            state.session.sandbox_violation_flash_until = Some(
                state
                    .session
                    .sandbox_violation_flash_until
                    .map_or(deadline, |existing| existing.max(deadline)),
            );
            state
                .ui
                .toast_warning(t!("toast.sandbox_violations", count = params.count).to_string());
        }
        ServerNotification::StreamStallDetected(_) => {
            state.ui.toast_warning(t!("toast.stream_stall").to_string());
        }
        ServerNotification::StreamWatchdogWarning(params) => {
            state.ui.toast_warning(format!(
                "{} ({}s)",
                t!("toast.stream_watchdog"),
                params.elapsed_secs
            ));
        }
        ServerNotification::StreamRequestEnd(params) => {
            if let Some(reasoning_tokens) = params.usage.reasoning_tokens {
                state.session.add_thinking_tokens(reasoning_tokens as i32);
            }
            state.session.update_tokens_from_protocol(&params.usage);
        }
    }
}

/// Extract a short description from tool input JSON.
fn extract_tool_description(input: &str) -> String {
    const MAX_LEN: usize = 80;
    if input.is_empty() {
        return String::new();
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(input) {
        for key in [
            "command",
            "file_path",
            "pattern",
            "query",
            "description",
            "content",
        ] {
            if let Some(s) = v.get(key).and_then(serde_json::Value::as_str) {
                return if s.len() > MAX_LEN {
                    format!(
                        "{}…",
                        &s[..s
                            .char_indices()
                            .take_while(|(i, _)| *i < MAX_LEN)
                            .last()
                            .map_or(0, |(i, c)| i + c.len_utf8())]
                    )
                } else {
                    s.to_string()
                };
            }
        }
    }
    if input.len() > MAX_LEN {
        let end = input
            .char_indices()
            .take_while(|(i, _)| *i < MAX_LEN)
            .last()
            .map_or(0, |(i, c)| i + c.len_utf8());
        format!("{}…", &input[..end])
    } else {
        input.to_string()
    }
}
