//! Agent event handler.
//!
//! Processes [`LoopEvent`]s from the core agent loop and updates
//! [`AppState`] accordingly. Extracted from `update.rs` for module
//! size management.

use cocode_protocol::LoopEvent;
use cocode_protocol::McpStartupStatus;
use cocode_protocol::RewindMode;
use cocode_protocol::ToolResultContent;

use crate::i18n::t;
use crate::state::AppState;
use crate::state::ChatMessage;
use crate::state::InlineToolCall;
use crate::state::Overlay;
use crate::state::PermissionOverlay;
use crate::state::ToolStatus;

/// Handle an event from the core agent loop.
///
/// This function processes events from the agent and updates the
/// application state accordingly. It handles streaming content,
/// tool execution updates, and other agent lifecycle events.
///
/// Every [`LoopEvent`] variant is matched explicitly so that adding
/// a new variant causes a compile error rather than being silently
/// swallowed by a wildcard arm.
pub fn handle_agent_event(state: &mut AppState, event: LoopEvent) {
    match event {
        // ========== Turn Lifecycle ==========
        LoopEvent::TurnStarted {
            turn_id,
            turn_number,
        } => {
            state.ui.start_streaming(turn_id);
            state.ui.clear_thinking_duration();
            state.session.reset_thinking_tokens();
            state.session.current_turn_number = Some(turn_number);
            if let Some(msg) = state.session.messages.last_mut()
                && msg.role == crate::state::MessageRole::User
                && msg.turn_number.is_none()
            {
                msg.turn_number = Some(turn_number);
            }
            state.ui.query_timing.start();
        }
        LoopEvent::TurnCompleted { turn_id, usage } => {
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
                // Reveal any remaining content before finalizing
                streaming.reveal_all();
                let mut message = ChatMessage::assistant(&turn_id, &streaming.content);
                if !streaming.thinking.is_empty() {
                    message.thinking = Some(streaming.thinking);
                }
                // Attach inline tool calls from streaming tool_uses, matching
                // with session tool_executions for elapsed time and status.
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
                    });
                }
                // Also attach completed tools from session that aren't already listed
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
                        });
                    }
                }
                message.turn_number = state.session.current_turn_number;
                message.complete();
                state.session.add_message(message);
            }
            if let Some(reasoning_tokens) = usage.reasoning_tokens {
                state.session.add_thinking_tokens(reasoning_tokens as i32);
            }
            state.session.update_tokens(usage);
        }

        // ========== Content Streaming ==========
        LoopEvent::TextDelta { delta, .. } => {
            if state.ui.is_thinking() {
                state.ui.stop_thinking();
            }
            state.ui.append_streaming(&delta);
        }
        LoopEvent::ThinkingDelta { delta, .. } => {
            state.ui.start_thinking();
            state.ui.append_streaming_thinking(&delta);
        }
        LoopEvent::ToolCallDelta { call_id, delta } => {
            state.ui.append_tool_call_delta(&call_id, &delta);
        }
        // Raw SSE event passthrough; used for debugging, not displayed.
        LoopEvent::StreamEvent { .. } => {}

        // ========== Stream Lifecycle ==========
        LoopEvent::StreamRequestStart => {
            tracing::debug!("Stream request started");
        }
        LoopEvent::StreamRequestEnd { usage } => {
            if let Some(reasoning_tokens) = usage.reasoning_tokens {
                state.session.add_thinking_tokens(reasoning_tokens as i32);
            }
            state.session.update_tokens(usage);
        }

        // ========== Tool Execution ==========
        LoopEvent::ToolUseQueued {
            call_id,
            name,
            input,
        } => {
            // Initialize with the full input JSON so even if ToolCallDelta events
            // arrived before this entry was created, we still have the input data.
            let input_str = serde_json::to_string(&input).unwrap_or_default();
            state
                .ui
                .add_streaming_tool_use_with_input(call_id, name, input_str);
        }
        LoopEvent::ToolUseStarted {
            call_id,
            name,
            batch_id,
        } => {
            state.ui.set_stream_mode_tool_use();
            state.ui.spinner_text = Some(name.clone());
            state.session.start_tool_with_batch(call_id, name, batch_id);
        }
        LoopEvent::ToolProgress { call_id, progress } => {
            if let Some(msg) = progress.message {
                state.session.update_tool_progress(&call_id, msg);
            }
        }
        LoopEvent::ToolUseCompleted {
            call_id,
            output,
            is_error,
        } => {
            let output_str = match output {
                ToolResultContent::Text(s) => s,
                ToolResultContent::Structured(v) => v.to_string(),
            };
            state.session.complete_tool(&call_id, output_str, is_error);
            state.session.cleanup_completed_tools(10);
            state.ui.spinner_text = None;
        }
        LoopEvent::ToolExecutionAborted { reason } => {
            state
                .ui
                .toast_warning(format!("{}: {reason}", t!("toast.tool_aborted")));
        }

        // ========== Permission ==========
        LoopEvent::ApprovalRequired { request } => {
            state.ui.query_timing.on_permission_dialog_open();
            if request.tool_name == cocode_protocol::ToolName::ExitPlanMode.as_str() {
                state.ui.set_overlay(Overlay::PlanExitApproval(
                    crate::state::PlanExitOverlay::new(request),
                ));
            } else {
                state
                    .ui
                    .set_overlay(Overlay::Permission(PermissionOverlay::new(request)));
            }
        }
        // Echo of user's approval decision; already processed by overlay.
        LoopEvent::ApprovalResponse { .. } => {}
        // Permission check result; informational only.
        LoopEvent::PermissionChecked { tool, decision } => {
            tracing::trace!(tool, ?decision, "Permission checked");
        }

        // ========== User Questions ==========
        LoopEvent::QuestionAsked {
            request_id,
            questions,
        } => {
            state.ui.query_timing.on_permission_dialog_open();
            state
                .ui
                .set_overlay(Overlay::Question(crate::state::QuestionOverlay::new(
                    request_id, &questions,
                )));
        }

        // ========== Elicitation ==========
        LoopEvent::ElicitationRequested {
            request_id,
            server_name,
            message,
            mode,
            schema,
            url,
        } => {
            state.ui.query_timing.on_permission_dialog_open();
            let overlay = crate::state::ElicitationOverlay::from_request(
                request_id,
                server_name,
                message,
                &mode,
                schema.as_ref(),
                url,
            );
            state.ui.set_overlay(Overlay::Elicitation(overlay));
        }

        // ========== Plan Mode ==========
        LoopEvent::PlanModeEntered { plan_file } => {
            state.session.plan_mode = true;
            state.session.permission_mode = cocode_protocol::PermissionMode::Plan;
            state.session.plan_file = plan_file;
        }
        LoopEvent::PlanModeExited { .. } => {
            state.session.plan_mode = false;
            state.session.plan_file = None;
            if state.session.permission_mode == cocode_protocol::PermissionMode::Plan {
                state.session.permission_mode = cocode_protocol::PermissionMode::Default;
            }
        }

        // ========== Context Cleared ==========
        LoopEvent::ContextCleared { new_mode } => {
            state.session.messages.clear();
            state.session.tool_executions.clear();
            state.session.subagents.clear();
            state.session.plan_mode = false;
            state.session.plan_file = None;
            state.session.permission_mode = new_mode;
            state.ui.scroll_offset = 0;
            state.ui.user_scrolled = false;
            tracing::info!(?new_mode, "Context cleared after plan exit");
        }

        // ========== Permission Mode ==========
        LoopEvent::PermissionModeChanged { mode } => {
            state.session.permission_mode = mode;
            state.session.plan_mode = mode == cocode_protocol::PermissionMode::Plan;
        }

        // ========== Subagent Events ==========
        LoopEvent::SubagentSpawned {
            agent_id,
            agent_type,
            description,
            color,
        } => {
            state
                .session
                .start_subagent(agent_id, agent_type, description, color);
        }
        LoopEvent::SubagentProgress { agent_id, progress } => {
            state.session.update_subagent_progress(&agent_id, progress);
        }
        LoopEvent::SubagentCompleted { agent_id, result } => {
            state.session.complete_subagent(&agent_id, result);
            state.session.cleanup_completed_subagents(5);
        }
        LoopEvent::SubagentBackgrounded {
            agent_id,
            output_file,
        } => {
            state.session.background_subagent(&agent_id, output_file);
        }

        // ========== Background Tasks ==========
        LoopEvent::BackgroundTaskStarted { task_id, task_type } => {
            state
                .session
                .start_background_task(task_id, task_type.clone());
            state.ui.toast_info(
                t!(
                    "toast.background_task_started",
                    task_type = format!("{task_type:?}")
                )
                .to_string(),
            );
        }
        LoopEvent::BackgroundTaskProgress { task_id, progress } => {
            if let Some(msg) = progress.message {
                state.session.update_background_task_progress(&task_id, msg);
            }
        }
        LoopEvent::BackgroundTaskCompleted { task_id, .. } => {
            state.session.complete_background_task(&task_id);
            state.session.cleanup_completed_background_tasks(5);
            state
                .ui
                .toast_success(t!("toast.background_task_completed").to_string());
        }
        LoopEvent::AllAgentsKilled { count, agent_ids } => {
            for id in &agent_ids {
                state.session.fail_subagent(id, "killed".to_string());
            }
            state
                .ui
                .toast_warning(t!("toast.agents_killed", count = count).to_string());
        }

        // ========== Errors ==========
        LoopEvent::Error { error } => {
            state.ui.query_timing.stop();
            state.ui.spinner_text = None;
            // Show error as toast (non-blocking) and add to chat as system message
            let error_text = format!("{}: {}", error.code, error.message);
            state.ui.toast_error(error_text.clone());
            let mut msg = ChatMessage::system(format!("error-{}", error.code), &error_text);
            msg.turn_number = state.session.current_turn_number;
            state.session.add_message(msg);
        }
        LoopEvent::Interrupted => {
            state.ui.stop_streaming();
            state.ui.query_timing.stop();
            tracing::info!("Operation interrupted");
        }

        // ========== API / Retry ==========
        LoopEvent::Retry {
            attempt,
            max_attempts,
            ..
        } => {
            state
                .ui
                .toast_info(t!("toast.retry", attempt = attempt, max = max_attempts).to_string());
        }
        LoopEvent::ApiError { error, .. } => {
            state
                .ui
                .toast_error(t!("toast.api_error", message = error.message).to_string());
        }

        // ========== Context / Compaction ==========
        LoopEvent::ContextUsageWarning {
            percent_left,
            estimated_tokens,
            warning_threshold,
        } => {
            // Update context gauge in status bar
            state.session.context_window_used = estimated_tokens;
            state.session.context_window_total = warning_threshold;

            let format_tokens = |n: i32| -> String {
                if n >= 1_000_000 {
                    format!("{:.1}M", n as f64 / 1_000_000.0)
                } else if n >= 1_000 {
                    format!("{:.0}k", n as f64 / 1_000.0)
                } else {
                    n.to_string()
                }
            };
            let remain = format_tokens(warning_threshold.saturating_sub(estimated_tokens));
            let total = format_tokens(warning_threshold);
            let percent = (percent_left * 100.0) as i32;
            let msg = t!(
                "toast.context_warning",
                percent = percent,
                remain = remain,
                total = total
            )
            .to_string();
            state.ui.toast_warning(msg);
        }
        LoopEvent::CompactionStarted => {
            state.ui.toast_info(t!("toast.compacting").to_string());
            state.session.is_compacting = true;
            state.ui.spinner_text = Some(t!("toast.compacting").to_string());
        }
        LoopEvent::CompactionCompleted {
            removed_messages,
            summary_tokens,
        } => {
            let msg = t!(
                "toast.compacted",
                messages = removed_messages,
                tokens = summary_tokens
            )
            .to_string();
            state.ui.toast_success(msg);
            state.session.is_compacting = false;
            state.ui.spinner_text = None;
        }
        LoopEvent::CompactionFailed { error, .. } => {
            state
                .ui
                .toast_error(t!("toast.compaction_failed", error = error).to_string());
            state.session.is_compacting = false;
            state.ui.spinner_text = None;
        }
        LoopEvent::CompactionCircuitBreakerOpen {
            consecutive_failures,
        } => {
            state.ui.toast_warning(format!(
                "{} ({consecutive_failures})",
                t!("toast.compaction_circuit_breaker")
            ));
        }

        // Micro-compaction (trace-level except when applied)
        LoopEvent::MicroCompactionStarted { candidates, .. } => {
            tracing::debug!(candidates, "Micro-compaction started");
        }
        LoopEvent::MicroCompactionApplied {
            removed_results,
            tokens_saved,
        } => {
            state.ui.toast_info(
                t!(
                    "toast.micro_compaction",
                    count = removed_results,
                    tokens = tokens_saved
                )
                .to_string(),
            );
        }
        LoopEvent::SessionMemoryCompactApplied { saved_tokens, .. } => {
            state
                .ui
                .toast_info(t!("toast.session_memory_compact", saved = saved_tokens).to_string());
        }

        // Compaction details (debug logging only)
        LoopEvent::CompactionSkippedByHook {
            hook_name, reason, ..
        } => {
            tracing::debug!(hook_name, reason, "Compaction skipped by hook");
        }
        LoopEvent::CompactionRetry {
            attempt,
            max_attempts,
            reason,
            ..
        } => {
            tracing::debug!(attempt, max_attempts, reason, "Compaction retry");
        }
        LoopEvent::MemoryAttachmentsCleared {
            tokens_reclaimed, ..
        } => {
            tracing::debug!(tokens_reclaimed, "Memory attachments cleared");
        }
        LoopEvent::PostCompactHooksExecuted {
            hooks_executed,
            additional_context_count,
        } => {
            tracing::debug!(
                hooks_executed,
                additional_context_count,
                "Post-compact hooks executed"
            );
        }
        LoopEvent::CompactBoundaryInserted { trigger, .. } => {
            tracing::debug!(%trigger, "Compact boundary inserted");
        }
        LoopEvent::InvokedSkillsRestored { skills } => {
            tracing::debug!(?skills, "Invoked skills restored after compaction");
        }
        LoopEvent::ContextRestored {
            files_count,
            has_todos,
            has_plan,
        } => {
            tracing::debug!(files_count, has_todos, has_plan, "Context restored");
        }

        // ========== Session Memory Extraction ==========
        LoopEvent::SessionMemoryExtractionStarted { .. } => {
            state
                .ui
                .toast_info(t!("toast.session_memory_started").to_string());
        }
        LoopEvent::SessionMemoryExtractionCompleted { .. } => {
            state
                .ui
                .toast_success(t!("toast.session_memory_completed").to_string());
        }
        LoopEvent::SessionMemoryExtractionFailed { error, .. } => {
            tracing::error!(error, "Session memory extraction failed");
            state
                .ui
                .toast_error(t!("toast.session_memory_failed").to_string());
        }

        // ========== Model Fallback ==========
        LoopEvent::ModelFallbackStarted { from, to, reason } => {
            let msg = t!("toast.model_fallback", from = from, to = to).to_string();
            state.ui.toast_warning(msg);
            state.session.fallback_model = Some(to.clone());
            tracing::info!(from, to, reason, "Model fallback started");
        }
        LoopEvent::ModelFallbackCompleted => {
            state.session.fallback_model = None;
        }
        // Message tombstoned in conversation history; TUI uses its own message list.
        LoopEvent::Tombstone { .. } => {
            tracing::debug!("Message tombstoned");
        }

        // ========== Speculative Execution ==========
        LoopEvent::SpeculativeStarted {
            speculation_id,
            tool_calls,
        } => {
            tracing::debug!(speculation_id, ?tool_calls, "Speculative execution started");
        }
        LoopEvent::SpeculativeCommitted {
            speculation_id,
            committed_count,
        } => {
            tracing::debug!(
                speculation_id,
                committed_count,
                "Speculative execution committed"
            );
        }
        LoopEvent::SpeculativeRolledBack {
            reason,
            speculation_id,
            ..
        } => {
            tracing::warn!(speculation_id, reason, "Speculative execution rolled back");
            state
                .ui
                .toast_warning(t!("toast.speculative_rolled_back", reason = reason).to_string());
        }

        // ========== Prompt Cache ==========
        LoopEvent::PromptCacheHit { cached_tokens } => {
            tracing::debug!(cached_tokens, "Prompt cache hit");
        }
        LoopEvent::PromptCacheMiss => {
            tracing::debug!("Prompt cache miss");
        }

        // ========== Queue ==========
        LoopEvent::CommandQueued { id, preview } => {
            // Sync core-queued commands into TUI state (handles commands
            // queued by the core itself, not just user input).
            if !state.session.queued_commands.iter().any(|c| c.id == id) {
                state
                    .session
                    .queued_commands
                    .push(cocode_protocol::UserQueuedCommand {
                        id: id.clone(),
                        prompt: preview.clone(),
                        queued_at: chrono::Utc::now().timestamp_millis(),
                    });
            }
            tracing::debug!(id, preview, "Command queued (from core)");
        }
        LoopEvent::CommandDequeued { id } => {
            state.session.queued_commands.retain(|c| c.id != id);
            tracing::debug!(id, "Command dequeued");
        }
        LoopEvent::QueueStateChanged { queued } => {
            let local = state.session.queued_commands.len() as i32;
            if local != queued {
                tracing::warn!(
                    local,
                    core = queued,
                    "Queue count mismatch between TUI and core"
                );
            }
        }

        // ========== MCP Events ==========
        LoopEvent::McpToolCallBegin {
            call_id,
            server,
            tool,
        } => {
            state.session.start_mcp_tool_call(call_id, server, tool);
        }
        LoopEvent::McpToolCallEnd {
            call_id,
            server,
            tool,
            is_error,
        } => {
            state.session.complete_mcp_tool_call(&call_id, is_error);
            state.session.cleanup_completed_mcp_calls(10);
            if is_error {
                state.ui.toast_error(
                    t!("toast.mcp_tool_failed", server = server, tool = tool).to_string(),
                );
            }
        }
        LoopEvent::McpStartupUpdate { server, status } => match status {
            McpStartupStatus::Ready => {
                state
                    .ui
                    .toast_success(t!("toast.mcp_ready", server = server).to_string());
            }
            McpStartupStatus::Failed => {
                state
                    .ui
                    .toast_error(t!("toast.mcp_failed", server = server).to_string());
            }
            McpStartupStatus::Starting => {
                tracing::debug!(server, "MCP server starting");
            }
            McpStartupStatus::Connecting => {
                tracing::debug!(server, "MCP server connecting");
            }
        },
        LoopEvent::McpStartupComplete { servers, failed } => {
            if !servers.is_empty() {
                let count = servers.len();
                state
                    .ui
                    .toast_success(t!("toast.mcp_connected", count = count).to_string());
            }
            for (name, error) in failed {
                state
                    .ui
                    .toast_error(t!("toast.mcp_error", name = name, error = error).to_string());
            }
        }

        // ========== Plugin Data ==========
        LoopEvent::PluginDataReady {
            installed,
            marketplaces,
        } => {
            use crate::state::MarketplaceSummary;
            use crate::state::PluginSummary;

            let installed_items: Vec<PluginSummary> = installed
                .into_iter()
                .map(|p| PluginSummary {
                    name: p.name,
                    description: p.description,
                    version: p.version,
                    enabled: p.enabled,
                    scope: p.scope,
                    skills_count: p.skills_count,
                    hooks_count: p.hooks_count,
                    agents_count: p.agents_count,
                })
                .collect();
            let marketplace_items: Vec<MarketplaceSummary> = marketplaces
                .into_iter()
                .map(|m| MarketplaceSummary {
                    name: m.name,
                    source_type: m.source_type,
                    source: m.source,
                    auto_update: m.auto_update,
                    plugin_count: m.plugin_count,
                })
                .collect();
            state.ui.set_overlay(Overlay::PluginManager(
                crate::state::PluginManagerOverlay::new(
                    installed_items,
                    marketplace_items,
                    Vec::new(),
                ),
            ));
        }
        // PluginAgentsLoaded is intercepted in app.rs and never reaches here.
        LoopEvent::PluginAgentsLoaded { .. } => {
            tracing::debug!("PluginAgentsLoaded reached handler unexpectedly");
        }

        // ========== Output Styles ==========
        LoopEvent::OutputStylesReady { styles } => {
            use crate::state::OutputStylePickerItem;

            let items: Vec<OutputStylePickerItem> = styles
                .into_iter()
                .map(|s| OutputStylePickerItem {
                    name: s.name,
                    source: s.source,
                    description: s.description,
                })
                .collect();
            if items.is_empty() {
                state
                    .ui
                    .toast_info(t!("toast.no_output_styles").to_string());
            } else {
                state.ui.set_overlay(Overlay::OutputStylePicker(
                    crate::state::OutputStylePickerOverlay::new(items),
                ));
            }
        }

        // ========== Rewind ==========
        LoopEvent::RewindCompleted {
            rewound_turn,
            restored_files,
            mode,
            restored_prompt,
            ..
        } => {
            if mode != RewindMode::CodeOnly {
                while let Some(msg) = state.session.messages.last() {
                    if msg.turn_number.is_some_and(|n| n >= rewound_turn) {
                        state.session.messages.pop();
                    } else {
                        break;
                    }
                }
            }
            if let Some(prompt) = restored_prompt
                && !prompt.is_empty()
            {
                state.ui.input.set_text(&prompt);
            }
            if matches!(state.ui.overlay, Some(Overlay::RewindSelector(_))) {
                state.ui.clear_overlay();
            }
            state.ui.toast_success(
                t!(
                    "toast.rewind_success",
                    turn = rewound_turn,
                    files = restored_files
                )
                .to_string(),
            );
        }
        LoopEvent::RewindFailed { error } => {
            if matches!(state.ui.overlay, Some(Overlay::RewindSelector(_))) {
                state.ui.clear_overlay();
            }
            state
                .ui
                .toast_error(t!("toast.rewind_failed", error = error).to_string());
        }
        LoopEvent::RewindCheckpointsReady { checkpoints } => {
            if checkpoints.is_empty() {
                state
                    .ui
                    .toast_info(t!("toast.rewind_no_checkpoints").to_string());
            } else {
                let mut overlay = crate::state::RewindSelectorOverlay::new(checkpoints);
                overlay.needs_initial_diff_stats = true;
                state.ui.set_overlay(Overlay::RewindSelector(overlay));
            }
        }
        LoopEvent::DiffStatsReady { turn_number, stats } => {
            if let Some(Overlay::RewindSelector(ref mut rw)) = state.ui.overlay {
                for cp in &mut rw.checkpoints {
                    if cp.turn_number == turn_number {
                        cp.diff_stats = Some(stats);
                        break;
                    }
                }
            }
        }

        // ========== Summarize ==========
        LoopEvent::SummarizeCompleted {
            from_turn,
            summary_tokens: _,
        } => {
            if matches!(state.ui.overlay, Some(Overlay::RewindSelector(_))) {
                state.ui.clear_overlay();
            }
            state
                .ui
                .toast_success(t!("toast.summarize_success", turn = from_turn).to_string());
        }
        LoopEvent::SummarizeFailed { error } => {
            if matches!(state.ui.overlay, Some(Overlay::RewindSelector(_))) {
                state.ui.clear_overlay();
            }
            state
                .ui
                .toast_error(t!("toast.summarize_failed", error = error).to_string());
        }

        // ========== Hooks ==========
        LoopEvent::HookExecuted {
            hook_type,
            hook_name,
        } => {
            tracing::debug!(
                hook_type = %hook_type,
                hook_name = %hook_name,
                "Hook executed"
            );
        }

        // ========== System Reminders ==========
        LoopEvent::SystemReminderDisplay { reminder_type, .. } => {
            tracing::debug!(reminder_type, "System reminder displayed");
        }

        // ========== Stream Warnings ==========
        LoopEvent::StreamStallDetected { .. } => {
            state.ui.toast_warning(t!("toast.stream_stall").to_string());
        }
        LoopEvent::StreamWatchdogWarning { elapsed_secs } => {
            state
                .ui
                .toast_warning(format!("{} ({elapsed_secs}s)", t!("toast.stream_watchdog")));
        }

        // ========== Limits ==========
        LoopEvent::MaxTurnsReached => {
            state
                .ui
                .toast_info(t!("toast.max_turns_reached").to_string());
        }

        // ========== Cron ==========
        LoopEvent::CronJobFired { job_id, prompt, .. } => {
            tracing::info!(job_id, prompt, "Cron job fired");
        }
        LoopEvent::CronJobDisabled {
            job_id,
            consecutive_failures,
        } => {
            state.ui.toast_warning(
                t!(
                    "toast.cron_job_disabled",
                    job_id = job_id,
                    failures = consecutive_failures
                )
                .to_string(),
            );
        }
        LoopEvent::CronJobsMissed { count, summary } => {
            state.ui.toast_info(
                t!("toast.cron_jobs_missed", count = count, summary = summary).to_string(),
            );
        }
    }
}

/// Extract a short description from tool input JSON.
///
/// Tries to pull out the most meaningful field (command, file_path, pattern, etc.)
/// for display in inline tool calls. Falls back to truncated raw input.
fn extract_tool_description(input: &str) -> String {
    const MAX_LEN: usize = 80;
    if input.is_empty() {
        return String::new();
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(input) {
        // Try common tool input fields in priority order
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
    // Fallback: truncated raw input
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

#[cfg(test)]
#[path = "agent_event_handler.test.rs"]
mod tests;
