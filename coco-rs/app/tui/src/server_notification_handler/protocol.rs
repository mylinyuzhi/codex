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

use coco_messages::SystemMessageLevel;
use coco_types::ServerNotification;

use crate::command::SystemPushKind;
use crate::i18n::t;
use crate::state::AppState;
use crate::state::ModalState;
use crate::state::PanePromptState;
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

pub(super) fn handle(
    state: &mut AppState,
    notif: ServerNotification,
    command_tx: &tokio::sync::mpsc::Sender<crate::command::UserCommand>,
) -> bool {
    match notif {
        // === Session lifecycle ===
        ServerNotification::SessionStarted(p) => {
            state.session.session_id = Some(p.session_id);
            // Initialise thinking_effort from the model's registered
            // default so the status bar reflects the starting state
            // before any Ctrl+T cycle. Falls back to Auto when the
            // model has no opinion (the resting state).
            state.session.thinking_effort = coco_config::builtin_models_partial()
                .get(&p.model)
                .and_then(|info| info.default_thinking_level)
                .unwrap_or(coco_types::ReasoningEffort::Auto);
            state.session.model = p.model;
            // Resolve the current git branch before storing cwd so a
            // failure (cwd outside a repo, detached HEAD) leaves
            // `git_branch = None` rather than a stale value. Stdout
            // shells out to `git rev-parse`; this fires once per session
            // start so the cost is negligible.
            let cwd_path = std::path::PathBuf::from(&p.cwd);
            state.session.git_branch = coco_git::get_current_branch(&cwd_path).ok().flatten();
            state.session.working_dir = Some(p.cwd);
            // A new session invalidates any cached agent markdown — otherwise
            // /copy could surface text from a previous conversation.
            state.session.last_agent_markdown = None;
            // LSP active flag drives the status-bar badge. Stays sticky
            // for the lifetime of the session — a per-server refresh
            // would need a separate `LspPrewarmComplete` event (variant
            // is defined, emission TBD when settings reload triggers
            // re-prewarm).
            state.session.lsp_active = p.lsp_active;
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
            state.session.current_turn_started_at = Some(std::time::Instant::now());
            state.ui.streaming = Some(crate::state::ui::StreamingState::new());
            true
        }
        ServerNotification::TurnCompleted(p) => on_turn_completed(state, p, command_tx),
        ServerNotification::TurnFailed(p) => {
            state.session.set_busy(false);
            state.session.current_turn_started_at = None;
            state.ui.streaming = None;
            // Drop in-flight tool widgets (status=Queued/Running, no
            // stamped `message_uuid`): the turn failed before they
            // could resolve to a finalized `Message::ToolResult`, so
            // leaving them on screen renders ghost spinners that
            // outlast the turn. Stamped executions belong to prior
            // turns and stay.
            let before = state.session.tool_executions.len();
            state
                .session
                .tool_executions
                .retain(|t| t.message_uuid.is_some());
            let dropped = before.saturating_sub(state.session.tool_executions.len());
            if dropped > 0 {
                tracing::info!(
                    target: "coco_tui::turn",
                    dropped,
                    remaining = state.session.tool_executions.len(),
                    error = %p.error,
                    "TurnFailed: dropped in-flight tool widgets",
                );
            } else {
                tracing::warn!(
                    target: "coco_tui::turn",
                    error = %p.error,
                    "TurnFailed",
                );
            }
            // Keep the toast for session history / notification log, AND
            // raise a modal error dialog so users can't miss the failure
            // (PR-F1 P0). The toast auto-expires; the state blocks input
            // until dismissed.
            state.ui.add_toast(Toast::error(
                t!("toast.turn_failed_short", error = p.error.as_str()).to_string(),
            ));
            let body = crate::widgets::error_dialog::turn_failed_body(&p);
            state.ui.show_modal(ModalState::Error(body));
            true
        }
        ServerNotification::TurnInterrupted(p) => on_turn_interrupted(state, p, command_tx),
        // [F11 anchor] All remaining match arms below this point don't
        // dispatch follow-up UserCommands; they're pure state folds.
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
            state.ui.show_modal(ModalState::Error(body));
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
                started_at_ms: Some(crate::state::session::now_ms()),
                token_usage: None,
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
            // Surface the completion line in the teammate preview
            // (TS getMessagePreview includes the final assistant
            // message). Falls back to a canned status line when the
            // result is empty.
            let line = if p.result.is_empty() {
                if p.is_error {
                    t!("teammate.completed_failed").to_string()
                } else {
                    t!("teammate.completed_ok").to_string()
                }
            } else {
                p.result.clone()
            };
            push_teammate_message(state, &p.agent_id, &line, command_tx);
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
                // Push a teammate message onto the engine transcript so
                // the teammate spinner-line preview
                // (`showTeammateMessagePreview`) and transcript reader
                // can pick it up. Tagged as a meta system message so it
                // stays out of the regular chat scroll — surfaces in the
                // transcript reader and per-teammate preview only.
                push_teammate_message(state, &p.agent_id, msg, command_tx);
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

        // === LSP ===
        ServerNotification::LspPrewarmComplete(p) => {
            // Mid-session prewarm completed (e.g. settings reload).
            // Flip the badge based on whether the new prewarm spawned
            // anything. SessionStarted carries the bootstrap value;
            // this state handles subsequent state changes.
            state.session.lsp_active = !p.started.is_empty();
            true
        }

        // === Context ===
        ServerNotification::ContextCompacted(p) => {
            state.session.is_compacting = false;
            // Suppress the next ContextUsageWarning emission — the
            // freshly-compacted token count won't be reflected in the
            // banner until the next API response arrives. TS:
            // `services/compact/compactWarningHook.ts` subscribes
            // `compactWarningStore` to gate the warning.
            state.session.compact_warning_suppressed = true;
            state.ui.add_toast(Toast::info(
                t!("toast.compacted_short", count = p.removed_messages).to_string(),
            ));
            true
        }
        ServerNotification::ContextUsageWarning(p) => {
            state.session.context_usage_percent = Some(p.percent_left);
            // The next API response will deliver an accurate token count;
            // until then, drop the warning so we don't show a stale value.
            // TS: `compactWarningHook.ts` gates on `compactWarningStore`.
            if !state.session.compact_warning_suppressed && p.percent_left < 10.0 {
                state.ui.add_toast(Toast::warning(format!(
                    "Context {:.0}% remaining",
                    p.percent_left
                )));
            }
            // Any subsequent warning post-suppression means the API
            // already returned a fresh count — clear the flag.
            state.session.compact_warning_suppressed = false;
            true
        }
        ServerNotification::CompactionStarted => {
            state.session.is_compacting = true;
            state.session.compact_warning_suppressed = false;
            true
        }
        ServerNotification::CompactionPhase(p) => {
            use crate::state::session::CompactionPhaseLabel;
            use coco_types::CompactionHookType;
            use coco_types::CompactionPhase;
            state.session.compaction_phase = match (p.phase, p.hook_type) {
                (CompactionPhase::HooksStart, Some(CompactionHookType::PreCompact)) => {
                    state.session.is_compacting = true;
                    Some(CompactionPhaseLabel::PreCompactHooks)
                }
                (CompactionPhase::HooksStart, Some(CompactionHookType::PostCompact)) => {
                    Some(CompactionPhaseLabel::PostCompactHooks)
                }
                (CompactionPhase::HooksStart, Some(CompactionHookType::SessionStart)) => {
                    Some(CompactionPhaseLabel::SessionStartHooks)
                }
                (CompactionPhase::HooksStart, None) => Some(CompactionPhaseLabel::PreCompactHooks),
                (CompactionPhase::Summarizing, _) => {
                    state.session.is_compacting = true;
                    Some(CompactionPhaseLabel::Summarizing)
                }
                (CompactionPhase::Done, _) => {
                    state.session.is_compacting = false;
                    state.session.compact_warning_suppressed = true;
                    None
                }
            };
            true
        }
        ServerNotification::CompactionFailed(p) => {
            state.session.is_compacting = false;
            state.session.compaction_phase = None;
            // Compaction failures leave the session in a compromised state
            // (context still over budget); escalate past the toast.
            let msg = t!("toast.compaction_failed_short", error = p.error.as_str()).to_string();
            state.ui.add_toast(Toast::error(msg.clone()));
            let body =
                crate::widgets::error_dialog::format_error_body(&msg, Some("compaction"), false);
            state.ui.show_modal(ModalState::Error(body));
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
        ServerNotification::TaskPanelChanged(p) => {
            // Unified snapshot refresh — mirrors TS `notifyTasksUpdated`.
            state.session.plan_tasks = p.plan_tasks;
            state.session.todos_by_agent = p.todos_by_agent;
            state.session.expanded_view = p.expanded_view;
            state.session.verification_nudge_pending = p.verification_nudge_pending;
            true
        }
        ServerNotification::PlanApprovalRequested(p) => {
            let prompt = crate::state::PlanApprovalPromptState::new(
                p.request_id,
                p.from,
                p.plan_file_path,
                p.plan_content,
            );
            state.ui.push_prompt(PanePromptState::PlanApproval(prompt));
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
        ServerNotification::ModelRoleChanged(p) => {
            // Fold the binding into `model_by_role` (one entry per role
            // independent of provider/model_id so the picker's "current"
            // marker always points to the right row).
            state.session.model_by_role.insert(
                p.role,
                crate::state::ModelBinding {
                    model_id: p.model_id.clone(),
                    provider: p.provider.clone(),
                    effort: p.effort,
                },
            );
            // Mirror Main role into the legacy `model`/`provider`/
            // `thinking_effort` fields the status bar reads directly.
            // Non-Main roles only affect the picker view.
            if p.role == coco_types::ModelRole::Main {
                state.session.model = p.model_id.clone();
                state.session.provider = p.provider.clone();
                state.session.thinking_effort =
                    p.effort.unwrap_or(coco_types::ReasoningEffort::Auto);
            }
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
            // Route the capability gate so the TUI state + Shift+Tab
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
            // Non-retryable errors escalate to a modal state so the user
            // must acknowledge before continuing (PR-F1 P0).
            state.ui.add_toast(Toast::error(p.message.clone()));
            if !p.retryable {
                let body = crate::widgets::error_dialog::error_body(&p);
                state.ui.show_modal(ModalState::Error(body));
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
            // Reconciliation safety net: if per-item `CommandDequeued`
            // events were lost (channel saturation, fork dispatch
            // missed an emit) the engine's authoritative count clamps
            // the display so it never drifts to a stale, infinitely-
            // growing list.
            state
                .session
                .queued_commands
                .truncate(queued.max(0) as usize);
            true
        }
        ServerNotification::CommandQueued { id, preview } => {
            state
                .session
                .queued_commands
                .push_back(crate::state::session::QueuedCommandDisplay { id, preview });
            true
        }
        ServerNotification::CommandDequeued { id } => {
            // Match by id so priority reordering between enqueue and
            // drain doesn't cause us to remove the wrong preview. Falls
            // back to pop_front if the id isn't tracked locally — that
            // happens when a producer emits CommandQueued through a
            // path the TUI didn't observe (e.g. SDK batch enqueues
            // before the TUI subscribed).
            if let Some(pos) = state
                .session
                .queued_commands
                .iter()
                .position(|q| q.id == id)
            {
                state.session.queued_commands.remove(pos);
            } else if !state.session.queued_commands.is_empty() {
                state.session.queued_commands.pop_front();
            }
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
            // `CostWarning` state (already defined) so users can stop
            // or continue explicitly. Keep the toast for the event log.
            let toast_msg = format!(
                "Cost: ${:.2} / ${:.2} threshold",
                p.current_cost_cents as f64 / 100.0,
                p.threshold_cents as f64 / 100.0
            );
            state.ui.add_toast(Toast::warning(toast_msg));
            state.ui.push_prompt(PanePromptState::CostWarning(
                crate::state::CostWarningPromptState {
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
            state.ui.show_modal(ModalState::Error(body));
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
                && !p.stdout.is_empty()
            {
                hook.output = Some(p.stdout);
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
            state.ui.show_modal(ModalState::Error(body));
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
            state.ui.dismiss_prompt();
            true
        }
        ServerNotification::ToolUseSummary(p) => {
            // Side-cache the summary for the tool batch — UI-only
            // overlay polish, intentionally NOT written to
            // `MessageHistory` (I-3: UI-only state stays UI-only).
            // Anchor on the first `preceding_tool_use_id` so the
            // renderer can attach the label to the assistant turn
            // that initiated the batch.
            match p.preceding_tool_use_ids.first() {
                Some(anchor) => {
                    tracing::debug!(
                        target: "coco_tui::tool_use_summary",
                        anchor_call_id = %anchor,
                        tool_count = p.preceding_tool_use_ids.len(),
                        summary_chars = p.summary.len(),
                        cache_size_after = state.session.tool_group_summaries.len() + 1,
                        "tool-use summary cached",
                    );
                    state
                        .session
                        .tool_group_summaries
                        .insert(anchor.clone(), p.summary);
                }
                None => {
                    tracing::warn!(
                        target: "coco_tui::tool_use_summary",
                        summary_chars = p.summary.len(),
                        "tool-use summary dropped: empty preceding_tool_use_ids \
                         (engine produced a summary with no batch anchor)",
                    );
                }
            }
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

        // === Plugins ===
        // Surfaces the "Plugins changed. Run /reload-plugins to activate."
        // banner; the user must run `/reload-plugins` explicitly. TS
        // parity: `useManagePlugins.ts:293-300` (color: 'suggestion' is
        // the lightest priority — closest TUI analogue is the info toast).
        ServerNotification::PluginsChanged { reason: _ } => {
            state.ui.add_toast(Toast::info(
                "Plugins changed. Run /reload-plugins to activate.".to_string(),
            ));
            true
        }

        // === History lifecycle ===
        //
        // Engine MessageHistory authoritative-mutation events. These
        // feed `session.transcript`, which the renderer pipeline reads
        // exclusively.
        ServerNotification::MessageAppended { message, .. } => {
            // Plan §6.4 atomic finalize: when the appended message is
            // the assistant push that just ended the in-flight stream,
            // the cell now owns the content — drop the overlay so the
            // same text does not render twice for one frame. Subsequent
            // stream deltas inside the same turn re-create the
            // StreamingState lazily via `get_or_insert_with` in
            // `stream.rs`.
            //
            // Single-agent model: every assistant push is the current
            // stream. A future parallel-stream world would need an
            // anchor UUID on StreamingState + an engine-side pre-
            // allocated assistant UUID; no emitter for that exists
            // today so the role check is the correct minimal fix.
            let is_assistant = matches!(message.as_ref(), coco_messages::Message::Assistant(_));
            // D4: stamp any pending tool_executions whose `call_id`
            // matches a `ToolCall` content block in this assistant
            // message. Stamps the parent message UUID so the
            // MessageTruncated handler can retain in-flight executions
            // whose anchor survives the truncate.
            if is_assistant {
                state
                    .session
                    .stamp_tool_executions_with_assistant_uuid(&message);
            }
            state.session.transcript.on_message_appended(message);
            if is_assistant {
                state.ui.streaming = None;
            }
            true
        }
        ServerNotification::MessageTruncated { keep_count, .. } => {
            let n = keep_count.max(0) as usize;
            let cells_before = state.session.transcript.len();
            let reasoning_before = state.session.reasoning_metadata.len();
            let tool_widgets_before = state.session.tool_executions.len();
            state.session.transcript.on_message_truncated(n);
            // D4: drop only tool overlays whose anchor message no
            // longer survives. Unstamped executions (mid-stream, no
            // committed assistant message yet) are kept — they belong
            // to the live turn the user is interacting with. The
            // streaming overlay is the live-turn anchor and is always
            // cleared because truncate semantically ends the live turn.
            let surviving_uuids: std::collections::HashSet<uuid::Uuid> = state
                .session
                .transcript
                .cells()
                .iter()
                .map(|c| c.message_uuid)
                .collect();
            state
                .session
                .retain_tool_executions_for_messages(&surviving_uuids);
            // Prune side-caches keyed by message uuid so they don't
            // outlive their anchor message. `tool_group_summaries` is
            // keyed by `preceding_tool_use_id` (call id) so it stays;
            // it'll be cleared on session reset.
            state
                .session
                .reasoning_metadata
                .retain(|uuid, _| surviving_uuids.contains(uuid));
            state.ui.streaming = None;
            tracing::info!(
                target: "coco_tui::history",
                keep_count = n,
                cells_before,
                cells_after = state.session.transcript.len(),
                reasoning_cache_dropped = reasoning_before
                    .saturating_sub(state.session.reasoning_metadata.len()),
                tool_widgets_dropped = tool_widgets_before
                    .saturating_sub(state.session.tool_executions.len()),
                "MessageTruncated applied",
            );
            true
        }
        ServerNotification::SessionResetForResume { session_id, .. } => {
            // Plan §6.3: clear every piece of UI-only state that
            // belonged to the prior session before the burst of
            // MessageAppended replays the loaded history. Without this,
            // tool panels and streaming text from the previous run
            // leak into the resumed view.
            tracing::info!(
                target: "coco_tui::history",
                new_session_id = %session_id,
                cells_cleared = state.session.transcript.len(),
                tool_widgets_cleared = state.session.tool_executions.len(),
                tool_summaries_cleared = state.session.tool_group_summaries.len(),
                reasoning_cache_cleared = state.session.reasoning_metadata.len(),
                "SessionResetForResume",
            );
            state.session.transcript.on_session_reset();
            state.session.tool_executions.clear();
            state.session.tool_group_summaries.clear();
            state.session.reasoning_metadata.clear();
            state.ui.streaming = None;
            // Conversation id rotates on resume so prompt-cache keys
            // do not collide with the prior run's break points.
            state.session.conversation_id = Some(session_id);
            true
        }
        ServerNotification::ReasoningMetadataAttached(p) => {
            let Ok(uuid) = uuid::Uuid::parse_str(&p.message_uuid) else {
                tracing::warn!(
                    target: "coco_tui::reasoning",
                    message_uuid = %p.message_uuid,
                    "ReasoningMetadataAttached: invalid UUID, dropping",
                );
                return true;
            };
            tracing::debug!(
                target: "coco_tui::reasoning",
                %uuid,
                reasoning_tokens = p.reasoning_tokens,
                duration_ms = ?p.duration_ms,
                "stamping reasoning metadata in side-cache (event-driven)",
            );
            state.session.reasoning_metadata.insert(
                uuid,
                crate::state::session::ReasoningMetadata {
                    duration_ms: p.duration_ms,
                    reasoning_tokens: p.reasoning_tokens,
                },
            );
            true
        }
        ServerNotification::HistoryReplaced { messages, .. } => {
            // Bulk resume hydration: a single replace instead of N
            // MessageAppended events. UI-only side-caches that anchor
            // on message uuids get cleared because the new transcript
            // overwrites the old.
            tracing::info!(
                target: "coco_tui::history",
                incoming = messages.len(),
                cells_before = state.session.transcript.len(),
                "HistoryReplaced (bulk hydration)",
            );
            state
                .session
                .transcript
                .replace_from_messages(messages.as_slice());
            state.session.tool_executions.clear();
            state.session.tool_group_summaries.clear();
            state.session.reasoning_metadata.clear();
            state.ui.streaming = None;
            tracing::debug!(
                target: "coco_tui::history",
                cells_after = state.session.transcript.len(),
                "HistoryReplaced applied",
            );
            true
        }
    }
}

/// Handle `TurnCompleted`: finalize usage, flush streaming buffer into the
/// message list, prune completed tools.
///
/// Does NOT handle auto-restore — that lives in [`on_turn_interrupted`].
/// `TurnCompleted` fires only on natural turn end; cancel paths emit
/// `TurnInterrupted` separately (see `app/cli/src/tui_runner.rs`).
fn on_turn_completed(
    state: &mut AppState,
    p: coco_types::TurnCompletedParams,
    _command_tx: &tokio::sync::mpsc::Sender<crate::command::UserCommand>,
) -> bool {
    state.session.set_busy(false);
    // TS REPL.tsx:3901 — `updateLastInteractionTime(true)` also fires
    // here so the idle window starts ticking from "user has had a
    // chance to read the response", not "agent stopped".
    let now = std::time::Instant::now();
    state.session.last_query_completion_at = Some(now);
    state.session.last_user_interaction_at = now;
    state.session.idle_prompt_fired = false;
    state.session.update_tokens(TokenUsage {
        input_tokens: p.usage.input_tokens,
        output_tokens: p.usage.output_tokens,
        reasoning_tokens: p.usage.reasoning_output_tokens(),
        cache_read_tokens: p.usage.cache_read_input_tokens(),
        cache_creation_tokens: p.usage.cache_creation_input_tokens(),
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
    // Telemetry-only — reasoning duration is now attached by the
    // engine via `ReasoningMetadataAttached`. Keep the local
    // computation for now in case future telemetry needs it.
    let _token_only_duration_ms: Option<i64> =
        state
            .session
            .current_turn_started_at
            .and_then(|started_at| started_at.elapsed().as_millis().try_into().ok())
            .or_else(|| {
                state.ui.streaming.as_ref().and_then(|streaming| {
                    streaming.started_at.elapsed().as_millis().try_into().ok()
                })
            });
    super::projection::flush_streaming_to_messages(state);
    // F3 — reasoning aggregates are stamped by the dedicated
    // `ReasoningMetadataAttached` handler (engine emits with the
    // assistant message UUID), so no cell-walk anchoring here.
    state.session.current_turn_started_at = None;
    state.session.tool_executions.retain(|t| {
        matches!(
            t.status,
            crate::state::session::ToolStatus::Queued | crate::state::session::ToolStatus::Running
        )
    });
    true
}

/// Handle `TurnInterrupted`: clear streaming state, surface the banner,
/// and run auto-restore when the cancel was user-initiated AND the idle
/// guards + lossless-tail predicate hold.
///
/// Mirrors TS `REPL.tsx:3010-3022` — the `.finally` block that fires
/// after `abortController.abort('user-cancel')` resolves the query.
fn on_turn_interrupted(
    state: &mut AppState,
    p: coco_types::TurnInterruptedParams,
    command_tx: &tokio::sync::mpsc::Sender<crate::command::UserCommand>,
) -> bool {
    state.session.set_busy(false);
    state.session.current_turn_started_at = None;
    state.ui.streaming = None;
    // Drop in-flight tool widgets — same rationale as `TurnFailed`.
    // The cancel aborts the turn before tools could resolve to a
    // `Message::ToolResult`, so any unstamped (= mid-turn) execution
    // would otherwise leak across the interrupt boundary.
    let before = state.session.tool_executions.len();
    state
        .session
        .tool_executions
        .retain(|t| t.message_uuid.is_some());
    let dropped = before.saturating_sub(state.session.tool_executions.len());
    tracing::info!(
        target: "coco_tui::turn",
        reason = ?p.reason,
        tool_widgets_dropped = dropped,
        tool_widgets_remaining = state.session.tool_executions.len(),
        "TurnInterrupted",
    );

    let user_cancel = matches!(p.reason, Some(coco_types::CancelReason::UserCancel));

    // Auto-restore is gated on:
    // - reason == UserCancel  → TS `signal.reason === 'user-cancel'`
    //   (treat None/legacy senders as non-user-initiated — conservative)
    // - empty input            → TS `inputValueRef.current === ''`
    // - empty queue            → TS `getCommandQueueLength() === 0`
    // - no state             → coco-rs analogue of "not viewing a
    //                            teammate task" + "no modal up"
    // - lossless tail          → TS `messagesAfterAreOnlySynthetic`
    // Predicates walk the engine-authoritative cell list directly.
    let cells = state.session.transcript.cells();
    let mut auto_restored = false;
    if user_cancel
        && state.ui.input.is_empty()
        && state.session.queued_commands.is_empty()
        && !state.ui.has_active_surface()
        && let Some(idx) = crate::update_rewind::find_last_user_cell_index(cells)
        && crate::update_rewind::cells_after_are_only_synthetic(cells, idx)
    {
        // Snapshot the index so we can mutate state below without
        // reborrowing `cells` (which would conflict with `state.ui`/
        // `state.session` mutations).
        apply_auto_restore(state, idx, command_tx);
        auto_restored = true;
    }

    // TS parity: `createUserInterruptionMessage` rendered as the dim
    // `Interrupted · What should Claude do instead?` chat row
    // (InterruptedByUser.tsx). Only fires for UserCancel — SystemPreempt
    // means a sibling op (Clear/Compact/Rewind/Shutdown) is about to
    // mutate history anyway. Skipped when auto-restore truncated to
    // the last user prompt: the prompt is now back in the input and
    // adding "you interrupted yourself" would be noise.
    //
    // The engine's `finalize_user_cancel` pushes a typed
    // `SystemMessage::UserInterruption` with the authoritative
    // `for_tool_use`; the MessageAppended event populates `transcript`,
    // and the renderer surfaces it from there.
    let _ = (user_cancel, auto_restored);
    true
}

/// In-place auto-restore — mirrors TS `restoreMessageSync`. Truncates
/// the message list at `idx` (the last user message), pops the user's
/// text back into the input bar, regenerates `conversation_id` so the
/// next turn starts a fresh cache key, and clears UI state that no
/// longer corresponds to a real conversation tail.
///
/// Dispatches `UserCommand::Rewind { mode: AutoRestore }` directly via
/// `command_tx.try_send`. The engine truncates its authoritative
/// history and emits `ServerNotification::MessageTruncated`, keeping
/// engine + TUI + SDK converged (see
/// `engine-tui-unified-transcript-plan.md` §7.4).
fn apply_auto_restore(
    state: &mut AppState,
    idx: usize,
    command_tx: &tokio::sync::mpsc::Sender<crate::command::UserCommand>,
) {
    let cells = state.session.transcript.cells();
    let Some(cell) = cells.get(idx) else {
        tracing::warn!(
            target: "coco_tui::auto_restore",
            idx,
            cells_len = cells.len(),
            "apply_auto_restore: cell index out of bounds — skipping",
        );
        return;
    };
    let target_message_id = cell.message_uuid.to_string();
    let input_text = match &cell.kind {
        crate::state::transcript_view::CellKind::UserText { text } => text.clone(),
        _ => String::new(),
    };
    let perm = match cell.source.as_ref() {
        coco_messages::Message::User(u) => u.permission_mode,
        _ => None,
    };
    // Phase 3d (§5): the renderer reads from `transcript.cells()`
    // directly. The engine emits `MessageTruncated` after our follow-up
    // `UserCommand::Rewind { mode: AutoRestore }` dispatch, which truncates
    // `transcript` to the same boundary.
    tracing::info!(
        target: "coco_tui::auto_restore",
        target_message_id = %target_message_id,
        cell_idx = idx,
        input_chars = input_text.len(),
        permission_mode = ?perm,
        "apply_auto_restore: queueing Rewind AutoRestore dispatch",
    );
    if let Some(mode) = perm {
        state.session.permission_mode = mode;
    }
    if !input_text.is_empty() {
        state.ui.input.textarea.set_text(&input_text);
        let eol = state.ui.input.textarea.end_of_current_line();
        state.ui.input.textarea.set_cursor(eol);
    }
    state.session.conversation_id = Some(uuid::Uuid::new_v4().to_string());
    state.session.prompt_suggestions.clear();
    state.ui.paste_manager.clear();
    state.ui.scroll_offset = 0;
    state.ui.user_scrolled = false;
    // Direct dispatch (no `pending_*` round-trip). `try_send` rather
    // than blocking `send` — the channel has slack; if it's full the
    // event loop is wedged for unrelated reasons and a dropped
    // auto-restore is the right fallback.
    if let Err(e) = command_tx.try_send(crate::command::UserCommand::Rewind {
        message_id: target_message_id.clone(),
        mode: crate::command::RewindMode::AutoRestore,
    }) {
        tracing::warn!(
            target: "coco_tui::auto_restore",
            target_message_id = %target_message_id,
            error = ?e,
            "apply_auto_restore: failed to dispatch Rewind AutoRestore",
        );
    }
}

/// Queue a teammate-attributed message for engine round-trip so the
/// per-teammate spinner-line preview (`UiState::show_teammate_message_preview`)
/// and the transcript state can surface it. Empty / whitespace-only
/// content is dropped so progress pings without a body don't pollute
/// the preview. Routed as `SystemMessage::Informational` with a
/// `teammate:<agent>` title so the renderer can distinguish the row.
fn push_teammate_message(
    _state: &mut AppState,
    agent_id: &str,
    content: &str,
    command_tx: &tokio::sync::mpsc::Sender<crate::command::UserCommand>,
) {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Err(e) = command_tx.try_send(crate::command::UserCommand::PushSystemMessage {
        kind: SystemPushKind::Informational {
            level: SystemMessageLevel::Info,
            title: format!("teammate:{agent_id}"),
            message: trimmed.to_string(),
        },
    }) {
        tracing::warn!(
            target: "coco_tui::teammate_message",
            %agent_id,
            error = ?e,
            "push_teammate_message: failed to dispatch PushSystemMessage",
        );
    }
}

#[cfg(test)]
#[path = "protocol.test.rs"]
mod tests;
