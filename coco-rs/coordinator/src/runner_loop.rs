//! In-process teammate execution loop.
//!
//! TS: utils/swarm/inProcessRunner.ts (1552 lines)
//!
//! Manages the multi-turn agent loop for in-process teammates:
//! prompt → run query → idle → wait for next prompt → loop.
//!
//! The actual LLM query execution is abstracted behind the
//! [`AgentExecutionEngine`] trait, allowing `app/query` to provide
//! the implementation without circular dependencies.
//!
//! ## Module split (P1)
//!
//! Helper functions live in sibling modules to keep this file under
//! the 800-LoC cap:
//!
//! - [`crate::runner_loop_mailbox_permission`] — cross-process
//!   permission via mailbox IPC + `MailboxPermissionBridge` impl.
//! - [`crate::runner_loop_wait`] — `wait_for_plan_approval`.
//! - [`crate::runner_loop_notify`] — outbound notification helpers
//!   (`send_message_to_leader`, `send_idle_notification`,
//!   `format_task_as_prompt`, `find_available_task`).

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use async_trait::async_trait;

use crate::constants::TEAM_LEAD_NAME;
use crate::mailbox;
use crate::pane::SystemPromptMode;
use crate::prompt;
use crate::task::InProcessTeammateTaskState;
use crate::task::TaskMessage;
use crate::teammate;
use crate::types::TeammateIdentity;

// ── Trait: AgentExecutionEngine ──

/// Configuration for a single query/turn within the teammate loop.
#[derive(Debug, Clone, Default)]
pub struct AgentQueryConfig {
    /// System prompt (base + addendum).
    pub system_prompt: String,
    /// Model override.
    pub model: Option<String>,
    /// Maximum turns for this query.
    pub max_turns: Option<i32>,
    /// Tools the agent is allowed to use without prompting.
    pub allowed_tools: Vec<String>,
    /// Tools explicitly denied to the agent regardless of allow-list.
    /// Sourced from the role's `disallowed_tools`. Threaded onward into
    /// the engine-side `coco_tool_runtime::AgentQueryConfig` so Layer 4
    /// of the filter pipeline narrows correctly.
    pub disallowed_tools: Vec<String>,
    /// Prior conversation messages (for context forking).
    pub fork_context_messages: Vec<serde_json::Value>,
    /// Whether to preserve full tool results (not previews).
    pub preserve_tool_use_results: bool,
    /// Parent session's bypass-permissions capability. Forwarded to the
    /// teammate's `ToolPermissionContext.bypass_available` so in-process
    /// teammates observe the same cycle + plan-exit gate as the leader.
    /// TS parity: `spawnUtils.ts:53` forwards
    /// `--dangerously-skip-permissions` to spawned children; the
    /// in-process analog is this field.
    pub bypass_permissions_available: bool,
    /// Parent session's resolved Layer 1 features. The engine bridge
    /// must thread this into `coco_tool_runtime::AgentQueryConfig.features`
    /// or teammates silently get registry defaults instead of the user's
    /// runtime resolution.
    pub features: Option<std::sync::Arc<coco_types::Features>>,
    /// Parent session's resolved Layer 2 tool overrides. Same rationale
    /// as `features`: a `None` here causes the engine bridge to fall
    /// back to `ToolOverrides::none()`, widening the set the active
    /// model accepts.
    pub tool_overrides: Option<std::sync::Arc<coco_types::ToolOverrides>>,
    /// Parent session's Layer 4 tool filter. The engine bridge feeds
    /// this into `coco_tool_runtime::AgentQueryConfig.parent_tool_filter`
    /// so the teammate's own allow/deny gets intersected via
    /// `ToolFilter::narrow_with`.
    pub parent_tool_filter: Option<coco_types::ToolFilter>,
}

/// Result from running a single query/turn.
#[derive(Debug, Clone)]
pub struct AgentQueryResult {
    /// Messages produced during this query.
    pub messages: Vec<serde_json::Value>,
    /// Total token count for this query.
    pub token_count: i64,
    /// Input tokens used.
    pub input_tokens: i64,
    /// Output tokens produced.
    pub output_tokens: i64,
    /// Number of turns executed.
    pub turns: i32,
    /// Number of tool uses.
    pub tool_use_count: i32,
    /// Whether the query was cancelled.
    pub cancelled: bool,
    /// Response text (last assistant message).
    pub response_text: Option<String>,
}

/// Abstraction over the query engine for teammate execution.
///
/// Implemented by `app/query` to avoid circular dependencies.
/// The runner loop calls `run_query()` for each turn of the
/// teammate's conversation.
#[async_trait]
pub trait AgentExecutionEngine: Send + Sync {
    /// Run a single agent query (one or more LLM turns).
    async fn run_query(
        &self,
        prompt: &str,
        config: AgentQueryConfig,
    ) -> anyhow::Result<AgentQueryResult>;

    /// Semantically compact the worker's message history when it grows
    /// past the auto-compact threshold. Implementers route through
    /// `coco-compact` to either run a full LLM compaction (replaces
    /// older turns with a summary) or a micro-compact (drops resolved
    /// tool results). The default returns the input unchanged so test
    /// engines and engines without compaction support don't have to
    /// implement it.
    ///
    /// Called by [`run_in_process_teammate`] at the tail of each turn
    /// when token usage exceeds [`InProcessRunnerConfig::auto_compact_threshold`].
    /// TS parity: `inProcessRunner.ts` runs `compactConversation` here
    /// rather than the sliding-window stopgap that lived in coco-rs
    /// before D1.
    async fn compact_messages(
        &self,
        messages: Vec<serde_json::Value>,
        _total_tokens: i64,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        // No-op default: keep the input. Real engines should override.
        Ok(messages)
    }
}

// ── Runner Config & Result ──

/// Configuration for running an in-process teammate.
///
/// TS: `InProcessRunnerConfig` in inProcessRunner.ts
#[derive(Debug, Clone)]
pub struct InProcessRunnerConfig {
    /// Teammate identity.
    pub identity: TeammateIdentity,
    /// Task ID in AppState.
    pub task_id: String,
    /// Initial prompt/task.
    pub prompt: String,
    /// Model override.
    pub model: Option<String>,
    /// Base system prompt.
    pub system_prompt: Option<String>,
    /// System prompt assembly mode.
    pub system_prompt_mode: SystemPromptMode,
    /// Tools allowed without permission prompts.
    pub allowed_tools: Vec<String>,
    /// Whether to allow permission prompts for unlisted tools.
    pub allow_permission_prompts: bool,
    /// Maximum turns per query.
    pub max_turns: Option<i32>,
    /// Cancellation flag (shared with AgentContext).
    pub cancelled: Arc<AtomicBool>,
    /// Auto-compact threshold (token count).
    pub auto_compact_threshold: i64,
    /// Parent session's bypass-permissions capability (forwarded on
    /// every query). Defaults to `false` so legacy callers stay safe.
    pub bypass_permissions_available: bool,
    /// Parent session's resolved Layer 1 features. Threaded into every
    /// teammate query config so in-process teammates see the same gate
    /// set as the leader.
    pub features: Option<std::sync::Arc<coco_types::Features>>,
    /// Parent session's resolved Layer 2 tool overrides. Same rationale.
    pub tool_overrides: Option<std::sync::Arc<coco_types::ToolOverrides>>,
    /// Parent session's Layer 4 tool filter — see `AgentQueryConfig`.
    pub parent_tool_filter: Option<coco_types::ToolFilter>,
    /// Whether the leader must approve the teammate's plan before any
    /// implementation turn runs. When `true`, after the first turn (the
    /// plan-write turn) the runner sends a
    /// [`mailbox::ProtocolMessage::PlanApprovalRequest`] to the leader
    /// and blocks via [`wait_for_plan_approval`] until a matching
    /// [`mailbox::ProtocolMessage::PlanApprovalResponse`] arrives. On
    /// rejection the loop continues with the leader's feedback as the
    /// next prompt; on approval the gate drops permanently for the
    /// remainder of the session. TS parity: `inProcessRunner.ts`
    /// plan-mode-entry hook.
    pub plan_mode_required: bool,
}

/// Result from running an in-process teammate to completion.
///
/// TS: `InProcessRunnerResult`
#[derive(Debug, Clone)]
pub struct InProcessRunnerResult {
    pub success: bool,
    pub error: Option<String>,
    pub output: Option<String>,
    pub turns: i32,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
}

// ── Wait Result ──

/// Result from waiting for the next prompt or shutdown.
///
/// TS: `WaitResult` in waitForNextPromptOrShutdown()
#[derive(Debug, Clone)]
pub enum WaitResult {
    /// Shutdown requested by leader.
    ShutdownRequest { original_text: String },
    /// New message received (from leader, peer, or task list).
    NewMessage {
        message: String,
        from: String,
        color: Option<String>,
        summary: Option<String>,
    },
    /// Agent was aborted (lifecycle cancellation).
    Aborted,
}

// ── Main Execution Loop ──

/// Run an in-process teammate to completion.
///
/// TS: `runInProcessTeammate(config)` — the main 500-line loop.
///
/// Flow:
/// 1. Build system prompt (base + addendum per mode)
/// 2. Try to claim an initial task from the task list
/// 3. Loop: run query → transition to idle → wait for next prompt
/// 4. Handle compaction when token count exceeds threshold
/// 5. Update task state at each lifecycle phase
/// 6. Send idle notification on idle transition
/// 7. Cleanup on exit (completion, failure, or abort)
pub async fn run_in_process_teammate(
    config: InProcessRunnerConfig,
    engine: &dyn AgentExecutionEngine,
    task_state: &tokio::sync::RwLock<InProcessTeammateTaskState>,
) -> InProcessRunnerResult {
    // Build system prompt
    let system_prompt = prompt::build_teammate_system_prompt(
        config.system_prompt.as_deref(),
        None,
        config.system_prompt_mode,
    );

    let mut all_messages: Vec<serde_json::Value> = Vec::new();
    let mut current_prompt = config.prompt.clone();
    let mut total_turns = 0i32;
    let mut total_input_tokens = 0i64;
    let mut total_output_tokens = 0i64;
    let mut was_idle = false;
    // `Some(msg)` once a query failure has been observed — the unified
    // cleanup path at the bottom uses this to flip the
    // `<task-notification>` status to `Failed` and the
    // `on_teammate_stop` reason to "failed: {msg}".
    let mut run_error: Option<String> = None;
    // Tracks whether the *current* turn was triggered by a shutdown
    // request from the leader. After the model finishes its response
    // (which becomes the shutdown approval/rejection text the leader
    // sees), we exit the loop cleanly. TS parity: `inProcessRunner.ts`
    // exits after the model finalises its shutdown reply rather than
    // looping for another message.
    let mut handling_shutdown = false;
    // Plan-approval gate — initially open iff the spawn did NOT request
    // plan-mode. When `true`, the runner suspends after each model turn
    // and pushes a `PlanApprovalRequest` to the leader, then awaits the
    // matching response via `wait_for_plan_approval` before continuing.
    // Once approved (or once the spawn never asked for plan-mode) the
    // flag stays `false` for the rest of the session. TS parity:
    // `inProcessRunner.ts` only gates between the plan-write turn and
    // the first implementation turn — so we drop the flag on the first
    // approval rather than re-arming.
    let mut plan_approval_pending = config.plan_mode_required;

    // Main loop
    loop {
        // Check cancellation
        if config.cancelled.load(Ordering::Relaxed) {
            break;
        }

        // Update task: mark as running
        {
            let mut state = task_state.write().await;
            state.is_idle = false;
            state.spinner_verb = Some("Working".to_string());
        }

        // Build query config
        let query_config = AgentQueryConfig {
            system_prompt: system_prompt.clone(),
            model: config.model.clone(),
            max_turns: config.max_turns,
            allowed_tools: config.allowed_tools.clone(),
            disallowed_tools: Vec::new(),
            fork_context_messages: all_messages.clone(),
            preserve_tool_use_results: true,
            bypass_permissions_available: config.bypass_permissions_available,
            features: config.features.clone(),
            tool_overrides: config.tool_overrides.clone(),
            parent_tool_filter: config.parent_tool_filter.clone(),
        };

        // Run query
        let query_result = match engine.run_query(&current_prompt, query_config).await {
            Ok(result) => result,
            Err(e) => {
                // Stash the error and break out so the unified cleanup at
                // the bottom of the function runs `on_teammate_stop` AND
                // the coordinator-mode `<task-notification>` send. Earlier
                // code returned directly here, skipping the
                // task-notification — coordinators wouldn't see workers
                // that errored on their first turn.
                let error_msg = format!("{e}");
                {
                    let mut state = task_state.write().await;
                    state.error = Some(error_msg.clone());
                }
                run_error = Some(error_msg);
                break;
            }
        };

        // Accumulate results
        total_turns += query_result.turns;
        total_input_tokens += query_result.input_tokens;
        total_output_tokens += query_result.output_tokens;
        all_messages.extend(query_result.messages.clone());

        // Update task progress
        {
            let mut state = task_state.write().await;
            state.turn_count = total_turns;
            state.input_tokens = total_input_tokens;
            state.output_tokens = total_output_tokens;
            state.tool_use_count += query_result.tool_use_count;

            // Append messages (capped)
            if let Some(text) = &query_result.response_text {
                state.append_message(TaskMessage {
                    role: "assistant".to_string(),
                    content: text.clone(),
                    tool_name: None,
                });
            }

            state.spinner_verb = None;
            state.past_tense_verb = Some("Completed".to_string());
        }

        // Check cancellation after query
        if config.cancelled.load(Ordering::Relaxed) || query_result.cancelled {
            break;
        }

        // Compaction check (D1) — runs at tail-of-turn, before idle
        // notification + wait_for_next_prompt, so the worker doesn't
        // sit idle holding a stale, oversized history (TS parity:
        // `inProcessRunner.ts:1072-1126` runs `compactConversation`
        // here as part of the post-tool tail).
        let total_tokens = total_input_tokens + total_output_tokens;
        if total_tokens > config.auto_compact_threshold && !all_messages.is_empty() {
            match engine
                .compact_messages(all_messages.clone(), total_tokens)
                .await
            {
                Ok(compacted) if compacted.len() < all_messages.len() => {
                    all_messages = compacted;
                }
                Ok(_) | Err(_) => {
                    // Engine declined / errored — apply the
                    // sliding-window safety valve so we don't grow
                    // unboundedly. 20 messages keeps recent context
                    // while bounding tokens.
                    let keep = all_messages.len().min(20);
                    let start = all_messages.len() - keep;
                    all_messages = all_messages[start..].to_vec();
                }
            }
        }

        // Shutdown enforcement: if this turn handled a shutdown request,
        // the model's response is the approval/rejection text. Exit the
        // loop so the cleanup path runs (TS parity — the loop does not
        // continue waiting for further messages once the shutdown
        // dialogue is complete).
        if handling_shutdown {
            break;
        }

        // Plan-approval gate — runs between the plan-write turn and the
        // first implementation turn. The model just produced its plan
        // (now in `query_result.response_text`); send it to the leader
        // and block until a matching `PlanApprovalResponse` arrives.
        // TS parity: `inProcessRunner.ts` plan-mode-entry hook.
        if plan_approval_pending {
            let plan_content = query_result.response_text.clone().unwrap_or_default();
            let request_id = uuid::Uuid::new_v4().to_string();
            let plan_envelope = mailbox::create_plan_approval_request_message(
                &config.identity.agent_name,
                &request_id,
                "",
                &plan_content,
            );
            let envelope_message = mailbox::TeammateMessage {
                from: config.identity.agent_name.clone(),
                text: plan_envelope,
                timestamp: chrono::Utc::now().to_rfc3339(),
                read: false,
                color: config
                    .identity
                    .color
                    .as_ref()
                    .map(|c| c.as_str().to_string()),
                summary: Some("plan approval request".to_string()),
            };
            let _ = mailbox::write_to_mailbox(
                TEAM_LEAD_NAME,
                envelope_message,
                &config.identity.team_name,
            );

            match crate::runner_loop_wait::wait_for_plan_approval(
                &config.identity,
                &config.cancelled,
                &request_id,
            )
            .await
            {
                None => break,
                Some((true, _)) => {
                    plan_approval_pending = false;
                }
                Some((false, feedback)) => {
                    // Plan rejected — re-prompt the model with the
                    // leader's feedback. The next turn writes a revised
                    // plan, which loops through this gate again.
                    let feedback_text = feedback
                        .unwrap_or_else(|| "Plan rejected by leader. Please revise.".to_string());
                    current_prompt = teammate::format_as_teammate_message(
                        TEAM_LEAD_NAME,
                        &feedback_text,
                        None,
                        Some("plan rejected"),
                    );
                    was_idle = false;
                    continue;
                }
            }
        }

        // Transition to idle
        if !was_idle {
            // Send idle notification to leader
            let idle_text = mailbox::create_idle_notification(
                &config.identity.agent_name,
                Some("available"),
                None,
            );
            let message = mailbox::TeammateMessage {
                from: config.identity.agent_name.clone(),
                text: idle_text,
                timestamp: chrono::Utc::now().to_rfc3339(),
                read: false,
                color: config
                    .identity
                    .color
                    .as_ref()
                    .map(|c| c.as_str().to_string()),
                summary: Some("idle".to_string()),
            };
            let _ = mailbox::write_to_mailbox(TEAM_LEAD_NAME, message, &config.identity.team_name);
        }

        {
            let mut state = task_state.write().await;
            state.is_idle = true;
        }
        // Mark as idle to suppress duplicate notifications on next iteration
        was_idle = true;

        // Read was_idle to prevent "value never read" warning.
        // The flag controls idle notification sending above.
        let _ = was_idle;

        // Wait for next prompt or shutdown
        let wait_result =
            wait_for_next_prompt_or_shutdown(&config.identity, &config.cancelled).await;

        match wait_result {
            WaitResult::Aborted => break,

            WaitResult::ShutdownRequest { original_text } => {
                // Pass shutdown request to model as input. The model
                // decides whether to approve or reject; the next-iteration
                // post-query check (`handling_shutdown`) will then exit
                // the loop so the cleanup path runs.
                let wrapped = teammate::format_as_teammate_message(
                    TEAM_LEAD_NAME,
                    &original_text,
                    None,
                    Some("shutdown request"),
                );
                current_prompt = wrapped;
                was_idle = false;
                handling_shutdown = true;

                {
                    let mut state = task_state.write().await;
                    state.shutdown_requested = true;
                    state.append_message(TaskMessage {
                        role: "user".to_string(),
                        content: "Shutdown requested".to_string(),
                        tool_name: None,
                    });
                }
            }

            WaitResult::NewMessage {
                message,
                from,
                color,
                summary,
            } => {
                // Wrap peer/leader messages in XML format
                let wrapped = if from == "user" {
                    message
                } else {
                    teammate::format_as_teammate_message(
                        &from,
                        &message,
                        color.as_deref(),
                        summary.as_deref(),
                    )
                };
                current_prompt = wrapped;
                was_idle = false;

                {
                    let mut state = task_state.write().await;
                    state.is_idle = false;
                    state.append_message(TaskMessage {
                        role: "user".to_string(),
                        content: format!("Message from {from}"),
                        tool_name: None,
                    });
                }
            }
        }

        // Compaction now runs post-turn at the top of the loop body —
        // see the moved block right after the cancellation check above.
    }

    // Unified cleanup path — runs whether the loop exited normally,
    // via cancellation, via shutdown-reply completion, or via query
    // error. Earlier code split the error case out as an early return
    // and skipped the coordinator-mode notification.
    let stop_reason = run_error.as_ref().map(|msg| format!("failed: {msg}"));
    teammate::on_teammate_stop(
        &config.identity.agent_name,
        &config.identity.team_name,
        config.identity.color.as_ref().map(|c| c.as_str()),
        stop_reason.as_deref(),
    );

    // Coordinator-mode notification: when the leader is operating as a
    // coordinator (`COCO_COORDINATOR_MODE=1` + `Feature::AgentTeams`),
    // push a `<task-notification>` XML envelope to the leader's mailbox
    // so the model receives the structured worker-termination signal
    // (TS `coordinatorMode.ts:130-152`). Status reflects the actual
    // outcome — `Completed` on clean exit, `Failed` on query error.
    if let Some(features) = config.features.as_deref()
        && coco_subagent::is_coordinator_mode(features)
    {
        let agent_id = format!(
            "{}@{}",
            config.identity.agent_name, config.identity.team_name
        );
        let (status, summary, result) = match run_error.as_deref() {
            Some(err) => (
                coco_subagent::TaskNotificationStatus::Failed,
                format!("Agent \"{}\" failed", config.identity.agent_name),
                Some(err),
            ),
            None => (
                coco_subagent::TaskNotificationStatus::Completed,
                format!("Agent \"{}\" completed", config.identity.agent_name),
                None,
            ),
        };
        let xml = coco_subagent::render_task_notification(&coco_subagent::TaskNotification {
            task_id: &agent_id,
            status,
            summary: &summary,
            result,
            usage: Some(coco_subagent::TaskNotificationUsage {
                total_tokens: total_input_tokens + total_output_tokens,
                tool_uses: 0,
                duration_ms: 0,
            }),
        });
        let _ = crate::runner_loop_notify::send_message_to_leader(
            &config.identity.agent_name,
            &xml,
            config.identity.color.as_ref().map(|c| c.as_str()),
            &config.identity.team_name,
        );
    }

    {
        let mut state = task_state.write().await;
        state.is_idle = true;
        state.spinner_verb = None;
    }

    let success = run_error.is_none();
    InProcessRunnerResult {
        success,
        error: run_error,
        output: None,
        turns: total_turns,
        total_input_tokens,
        total_output_tokens,
    }
}

// ── Wait For Next Prompt ──

/// Poll interval for mailbox scanning (ms).
///
/// TS: 500ms in waitForNextPromptOrShutdown()
const POLL_INTERVAL_MS: u64 = 500;

/// Wait for the next prompt, shutdown request, or abort.
///
/// TS: `waitForNextPromptOrShutdown()` in inProcessRunner.ts
///
/// Priority order:
/// 1. Abort signal check.
/// 2. Mailbox messages:
///    a. Shutdown requests (highest mailbox priority).
///    b. Team-lead messages (second — represents user intent).
///    c. Peer messages (FIFO, third).
/// 3. Unclaimed tasks from task list (lowest).
///
/// **TS-parity gap, intentional**: TS additionally drains
/// `task.pendingUserMessages` (`inProcessRunner.ts:705-739`) — messages
/// the user typed into the teammate's transcript-view UI. coco-rs has
/// no transcript-view UI yet, so the queue's only producer doesn't
/// exist. When the TUI lands, the right port is an
/// `mpsc::UnboundedReceiver<String>` registered per-`agent_id` on
/// `InProcessAgentRunner`, drained at the top of this loop above the
/// abort check.
pub async fn wait_for_next_prompt_or_shutdown(
    identity: &TeammateIdentity,
    cancelled: &AtomicBool,
) -> WaitResult {
    let mut poll_count = 0u64;

    loop {
        // Sleep (skip first iteration for responsiveness)
        if poll_count > 0 {
            tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
        }
        poll_count += 1;

        // Priority 1: Abort check
        if cancelled.load(Ordering::Relaxed) {
            return WaitResult::Aborted;
        }

        // Priority 2: Mailbox scanning
        let messages =
            mailbox::read_mailbox(&identity.agent_name, &identity.team_name).unwrap_or_default();

        // 2a: Shutdown requests (highest mailbox priority)
        for (i, msg) in messages.iter().enumerate() {
            if msg.read {
                continue;
            }
            if mailbox::is_structured_protocol_message(&msg.text)
                && let Some(protocol) = mailbox::parse_protocol_message(&msg.text)
                && matches!(protocol, mailbox::ProtocolMessage::ShutdownRequest { .. })
            {
                let _ = mailbox::mark_message_as_read_by_index(
                    &identity.agent_name,
                    &identity.team_name,
                    i,
                );
                return WaitResult::ShutdownRequest {
                    original_text: msg.text.clone(),
                };
            }
        }

        // 2b: Team-lead messages (second priority)
        if let Some((i, msg)) = messages
            .iter()
            .enumerate()
            .find(|(_, m)| !m.read && m.from == TEAM_LEAD_NAME)
        {
            let _ = mailbox::mark_message_as_read_by_index(
                &identity.agent_name,
                &identity.team_name,
                i,
            );
            return WaitResult::NewMessage {
                message: msg.text.clone(),
                from: msg.from.clone(),
                color: msg.color.clone(),
                summary: msg.summary.clone(),
            };
        }

        // 2c: Any unread message (peer FIFO, third priority)
        if let Some((i, msg)) = messages.iter().enumerate().find(|(_, m)| !m.read) {
            let _ = mailbox::mark_message_as_read_by_index(
                &identity.agent_name,
                &identity.team_name,
                i,
            );
            return WaitResult::NewMessage {
                message: msg.text.clone(),
                from: msg.from.clone(),
                color: msg.color.clone(),
                summary: msg.summary.clone(),
            };
        }

        // Priority 3: Unclaimed tasks (lowest)
        // Task list polling would go here if task list crate were available.
        // For now, tasks are delivered via mailbox messages.
    }
}

// ── Task Management Helpers ──

/// Send a message to the team leader's mailbox.
///
/// Fire-and-forget entry point for starting a teammate.
///
/// TS: `startInProcessTeammate(config)` — calls runInProcessTeammate in background.
pub fn start_in_process_teammate(
    config: InProcessRunnerConfig,
    engine: std::sync::Arc<dyn AgentExecutionEngine>,
    task_state: std::sync::Arc<tokio::sync::RwLock<InProcessTeammateTaskState>>,
) -> tokio::task::JoinHandle<InProcessRunnerResult> {
    tokio::spawn(async move { run_in_process_teammate(config, engine.as_ref(), &task_state).await })
}

#[cfg(test)]
#[path = "runner_loop.test.rs"]
mod tests;
