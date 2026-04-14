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

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use async_trait::async_trait;

use super::swarm::TeammateIdentity;
use super::swarm_backend::SystemPromptMode;
use super::swarm_constants::TEAM_LEAD_NAME;
use super::swarm_mailbox;
use super::swarm_prompt;
use super::swarm_task::InProcessTeammateTaskState;
use super::swarm_task::TaskMessage;
use super::swarm_teammate;

// ── Trait: AgentExecutionEngine ──

/// Configuration for a single query/turn within the teammate loop.
#[derive(Debug, Clone)]
pub struct AgentQueryConfig {
    /// System prompt (base + addendum).
    pub system_prompt: String,
    /// Model override.
    pub model: Option<String>,
    /// Maximum turns for this query.
    pub max_turns: Option<i32>,
    /// Tools the agent is allowed to use without prompting.
    pub allowed_tools: Vec<String>,
    /// Prior conversation messages (for context forking).
    pub fork_context_messages: Vec<serde_json::Value>,
    /// Whether to preserve full tool results (not previews).
    pub preserve_tool_use_results: bool,
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
    let system_prompt = swarm_prompt::build_teammate_system_prompt(
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
    let mut pending_user_messages: Vec<String> = Vec::new();

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
            fork_context_messages: all_messages.clone(),
            preserve_tool_use_results: true,
        };

        // Run query
        let query_result = match engine.run_query(&current_prompt, query_config).await {
            Ok(result) => result,
            Err(e) => {
                // Error: send failure notification and exit
                let error_msg = format!("{e}");
                swarm_teammate::on_teammate_stop(
                    &config.identity.agent_name,
                    &config.identity.team_name,
                    config
                        .identity
                        .color
                        .as_ref()
                        .map(super::swarm_constants::AgentColorName::as_str),
                    Some(&format!("failed: {error_msg}")),
                );

                let mut state = task_state.write().await;
                state.error = Some(error_msg.clone());
                state.is_idle = true;

                return InProcessRunnerResult {
                    success: false,
                    error: Some(error_msg),
                    output: None,
                    turns: total_turns,
                    total_input_tokens,
                    total_output_tokens,
                };
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

        // Transition to idle
        if !was_idle {
            // Send idle notification to leader
            let idle_text = swarm_mailbox::create_idle_notification(
                &config.identity.agent_name,
                Some("available"),
                None,
            );
            let message = swarm_mailbox::TeammateMessage {
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
            let _ = swarm_mailbox::write_to_mailbox(
                TEAM_LEAD_NAME,
                message,
                &config.identity.team_name,
            );
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
        let wait_result = wait_for_next_prompt_or_shutdown(
            &config.identity,
            &config.cancelled,
            &mut pending_user_messages,
        )
        .await;

        match wait_result {
            WaitResult::Aborted => break,

            WaitResult::ShutdownRequest { original_text } => {
                // Pass shutdown request to model as input
                // The model decides whether to approve or reject
                let wrapped = swarm_teammate::format_as_teammate_message(
                    TEAM_LEAD_NAME,
                    &original_text,
                    None,
                    Some("shutdown request"),
                );
                current_prompt = wrapped;
                was_idle = false;

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
                    swarm_teammate::format_as_teammate_message(
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

        // Compaction check
        let total_tokens = total_input_tokens + total_output_tokens;
        if total_tokens > config.auto_compact_threshold && !all_messages.is_empty() {
            // Clear accumulated messages (simplified compaction)
            // Full compaction would call the compact service, but that
            // lives in core/loop which we can't depend on.
            // Instead, keep only the last few messages as context.
            let keep = all_messages.len().min(20);
            let start = all_messages.len() - keep;
            all_messages = all_messages[start..].to_vec();
        }
    }

    // Cleanup
    swarm_teammate::on_teammate_stop(
        &config.identity.agent_name,
        &config.identity.team_name,
        config
            .identity
            .color
            .as_ref()
            .map(super::swarm_constants::AgentColorName::as_str),
        None,
    );

    {
        let mut state = task_state.write().await;
        state.is_idle = true;
        state.spinner_verb = None;
    }

    InProcessRunnerResult {
        success: true,
        error: None,
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
/// 1. Pending user messages (in-memory, from transcript view)
/// 2. Abort signal check
/// 3. Mailbox messages:
///    a. Shutdown requests (highest mailbox priority)
///    b. Team-lead messages (second — represents user intent)
///    c. Peer messages (FIFO, third)
/// 4. Unclaimed tasks from task list (lowest)
pub async fn wait_for_next_prompt_or_shutdown(
    identity: &TeammateIdentity,
    cancelled: &AtomicBool,
    pending_user_messages: &mut Vec<String>,
) -> WaitResult {
    let mut poll_count = 0u64;

    loop {
        // Priority 1: Pending user messages (in-memory queue)
        if let Some(message) = pending_user_messages.first().cloned() {
            pending_user_messages.remove(0);
            return WaitResult::NewMessage {
                message,
                from: "user".to_string(),
                color: None,
                summary: None,
            };
        }

        // Sleep (skip first iteration for responsiveness)
        if poll_count > 0 {
            tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
        }
        poll_count += 1;

        // Priority 2: Abort check
        if cancelled.load(Ordering::Relaxed) {
            return WaitResult::Aborted;
        }

        // Priority 3: Mailbox scanning
        let messages = swarm_mailbox::read_mailbox(&identity.agent_name, &identity.team_name)
            .unwrap_or_default();

        // 3a: Shutdown requests (highest mailbox priority)
        for (i, msg) in messages.iter().enumerate() {
            if msg.read {
                continue;
            }
            if swarm_mailbox::is_structured_protocol_message(&msg.text)
                && let Some(protocol) = swarm_mailbox::parse_protocol_message(&msg.text)
                && matches!(
                    protocol,
                    swarm_mailbox::ProtocolMessage::ShutdownRequest { .. }
                )
            {
                let _ = swarm_mailbox::mark_message_as_read_by_index(
                    &identity.agent_name,
                    &identity.team_name,
                    i,
                );
                return WaitResult::ShutdownRequest {
                    original_text: msg.text.clone(),
                };
            }
        }

        // 3b: Team-lead messages (second priority)
        if let Some((i, msg)) = messages
            .iter()
            .enumerate()
            .find(|(_, m)| !m.read && m.from == TEAM_LEAD_NAME)
        {
            let _ = swarm_mailbox::mark_message_as_read_by_index(
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

        // 3c: Any unread message (peer FIFO, third priority)
        if let Some((i, msg)) = messages.iter().enumerate().find(|(_, m)| !m.read) {
            let _ = swarm_mailbox::mark_message_as_read_by_index(
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

        // Priority 4: Unclaimed tasks (lowest)
        // Task list polling would go here if task list crate were available.
        // For now, tasks are delivered via mailbox messages.
    }
}

// ── Task Management Helpers ──

/// Send a message to the team leader's mailbox.
///
/// TS: `sendMessageToLeader(from, text, color, teamName)`
pub fn send_message_to_leader(
    from: &str,
    text: &str,
    color: Option<&str>,
    team_name: &str,
) -> anyhow::Result<()> {
    let message = swarm_mailbox::TeammateMessage {
        from: from.to_string(),
        text: text.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        read: false,
        color: color.map(String::from),
        summary: None,
    };
    swarm_mailbox::write_to_mailbox(TEAM_LEAD_NAME, message, team_name)
}

/// Send an idle notification to the leader.
///
/// TS: `sendIdleNotification(agentName, color, teamName, options?)`
pub fn send_idle_notification(
    agent_name: &str,
    color: Option<&str>,
    team_name: &str,
    idle_reason: Option<&str>,
    summary: Option<&str>,
) -> anyhow::Result<()> {
    let idle_text = swarm_mailbox::create_idle_notification(agent_name, idle_reason, summary);
    let message = swarm_mailbox::TeammateMessage {
        from: agent_name.to_string(),
        text: idle_text,
        timestamp: chrono::Utc::now().to_rfc3339(),
        read: false,
        color: color.map(String::from),
        summary: Some("idle notification".to_string()),
    };
    swarm_mailbox::write_to_mailbox(TEAM_LEAD_NAME, message, team_name)
}

/// Format a task as a prompt string.
///
/// TS: `formatTaskAsPrompt(task)`
pub fn format_task_as_prompt(task_id: &str, subject: &str, description: &str) -> String {
    let mut prompt = format!("Task #{task_id}: {subject}");
    if !description.is_empty() {
        prompt.push_str(&format!("\n\n{description}"));
    }
    prompt
}

/// Find the first available (unclaimed) task from a list.
///
/// TS: `findAvailableTask(tasks)`
pub fn find_available_task(tasks: &[super::TaskEntry]) -> Option<(usize, &super::TaskEntry)> {
    tasks
        .iter()
        .enumerate()
        .find(|(_, t)| t.status == "pending" && t.owner.is_none() && t.blocked_by.is_empty())
}

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
#[path = "swarm_runner_loop.test.rs"]
mod tests;
