//! Streaming tool executor for concurrent tool execution.
//!
//! This module provides [`StreamingToolExecutor`] which manages tool execution
//! during streaming, starting safe tools immediately and queuing unsafe tools.
//!
//! ## Hook Integration
//!
//! The executor supports hook execution at key lifecycle points:
//! - **PreToolUse**: Called before tool validation, can reject or modify input
//! - **PostToolUse**: Called after successful tool execution
//! - **PostToolUseFailure**: Called when a tool execution fails

use crate::ToolCall;
use crate::context::AgentContext;
use crate::context::ToolChannels;
use crate::context::ToolContext;
use crate::context::ToolEnvironment;
use crate::context::ToolServices;
use crate::context::ToolSharedState;
use crate::error::Result;
use crate::registry::ToolRegistry;
use crate::result_persistence;
use cocode_hooks::AsyncHookTracker;
use cocode_hooks::HookContext;
use cocode_hooks::HookEventType;
use cocode_hooks::HookRegistry;
use cocode_hooks::HookResult;
use cocode_protocol::AbortReason;
use cocode_protocol::CoreEvent;
use cocode_protocol::PermissionMode;
use cocode_protocol::StreamEvent;
use cocode_protocol::ToolOutput;
use cocode_protocol::TuiEvent;
use cocode_protocol::ValidationResult;
use cocode_protocol::server_notification::*;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::trace;
use tracing::warn;

// Re-export builder types at executor module level for backward compatibility.
pub use crate::executor_builder::ExecutorConfig;
pub(crate) use crate::executor_hooks::run_post_hooks;

/// Default maximum concurrent tool executions.
pub const DEFAULT_MAX_TOOL_CONCURRENCY: i32 = 10;

/// Action determined by post-hook execution.
///
/// Distinguishes between a hook rejecting the output (error) and a hook
/// providing a replacement output (non-error substitution).
pub(crate) enum PostHookAction {
    /// No post-hook intervened -- use the original result.
    None,
    /// A hook rejected the output -- replace with an error.
    Reject(String),
    /// A hook provided replacement output (ModifyOutput) -- use as-is.
    ReplaceOutput(ToolOutput),
    /// A hook requested that the agent loop stop after processing this tool result.
    /// The original output is preserved -- only the loop continuation is affected.
    StopContinuation(String),
}

/// Pending tool call waiting for execution.
#[derive(Debug)]
pub(crate) struct PendingToolCall {
    pub tool_call: ToolCall,
}

/// Consolidated executor state -- single lock for related execution data.
///
/// Previously 3 separate Mutexes, consolidated to prevent inconsistent
/// reads across related state.
pub(crate) struct ToolExecutionState {
    pub active_tasks: HashMap<String, JoinHandle<ToolExecutionResult>>,
    pub pending_unsafe: Vec<PendingToolCall>,
    pub completed_results: Vec<ToolExecutionResult>,
}

impl ToolExecutionState {
    pub fn new() -> Self {
        Self {
            active_tasks: HashMap::new(),
            pending_unsafe: Vec::new(),
            completed_results: Vec::new(),
        }
    }
}

/// Result from a tool execution.
#[derive(Debug)]
pub struct ToolExecutionResult {
    /// Tool call ID.
    pub call_id: String,
    /// Tool name.
    pub name: String,
    /// Execution result.
    pub result: Result<ToolOutput>,
    /// Additional contexts collected from pre-hook and post-hook `ContinueWithContext` results.
    pub additional_contexts: Vec<String>,
    /// When set, a PostToolUse hook requested that the agent loop stop after
    /// processing this tool result. Contains the reason/hook name.
    pub stop_continuation: Option<String>,
}

/// Streaming tool executor that manages concurrent tool execution.
///
/// This executor handles tool execution during streaming responses:
/// - Safe tools start immediately when their `ToolUse` block completes
/// - Unsafe tools are queued and executed sequentially after message_stop
///
/// ## Hook Integration
///
/// The executor supports hooks at key lifecycle points:
/// - **PreToolUse**: Before validation, can reject or modify input
/// - **PostToolUse**: After successful execution
/// - **PostToolUseFailure**: After failed execution
///
/// # Example
///
/// ```ignore
/// let executor = StreamingToolExecutor::new(registry, config, event_tx)
///     .with_hooks(hooks);
///
/// // During streaming - when content_block_stop for tool_use is received
/// executor.on_tool_complete(tool_call, ctx.clone());
///
/// // After message_stop - execute queued unsafe tools
/// executor.execute_pending_unsafe(&ctx).await;
///
/// // Wait for all tools to complete
/// let results = executor.drain().await;
/// ```
/// Lock Acquisition Order (deadlock prevention):
///
/// When acquiring multiple locks within the executor or tool pipeline,
/// always acquire in this order:
///
/// 1. `executor_state` (consolidated: active_tasks + pending_unsafe + completed_results)
/// 2. `state.approval_store`
/// 3. `state.file_tracker`
/// 4. `state.invoked_skills`
/// 5. `agent.agent_cancel_tokens`
/// 6. `agent.killed_agents`
/// 7. `state.output_offsets`
///
/// Within a single group (e.g., executor state), avoid holding one while
/// acquiring another -- operations should lock, read/write, then release.
pub struct StreamingToolExecutor {
    pub(crate) registry: Arc<ToolRegistry>,
    pub(crate) config: ExecutorConfig,
    pub(crate) event_tx: Option<mpsc::Sender<CoreEvent>>,
    pub(crate) cancel_token: CancellationToken,
    /// Hook registry for pre/post tool hooks.
    pub(crate) hooks: Option<Arc<HookRegistry>>,
    /// Tracker for async hooks running in background.
    pub(crate) async_hook_tracker: Arc<AsyncHookTracker>,
    /// Consolidated executor state (active tasks, pending unsafe, completed results).
    pub(crate) executor_state: Arc<Mutex<ToolExecutionState>>,
    /// Service handles (shell, sandbox, LSP, skills, permissions, etc.).
    pub(crate) services: ToolServices,
    /// Shared mutable state (approval store, file tracker, invoked skills, output offsets).
    pub(crate) state: ToolSharedState,
    /// Subagent-related context (spawn fn, cancel tokens, killed agents, output dir, etc.).
    pub(crate) agent: AgentContext,

    /// Current batch ID for parallel tool execution grouping (UI only).
    ///
    /// A fresh UUID is generated in [`set_allowed_tool_names`] at the start of
    /// each streaming turn. All tools emitted during that turn -- both safe
    /// (concurrent) and unsafe (sequential) -- share the same batch ID. The TUI
    /// uses this to visually group tools that were dispatched together; it does
    /// not affect execution ordering.
    pub(crate) current_batch_id: Arc<std::sync::RwLock<Option<String>>>,
    /// Cancellation token for sibling abort propagation.
    ///
    /// When a Bash tool fails (`is_error == true`) during parallel execution,
    /// this token is cancelled to abort all other concurrent sibling tools in
    /// the same batch. Reset at the start of each streaming turn by replacing
    /// the inner token. Matches Claude Code's `siblingAbortController` pattern.
    pub(crate) sibling_abort_token: Arc<std::sync::RwLock<CancellationToken>>,
    /// Description of the tool that triggered sibling abort (for error messages).
    pub(crate) sibling_error_desc: Arc<std::sync::RwLock<Option<String>>>,

    /// Allowlist of tool names the model was actually given.
    ///
    /// Set after `select_tools_for_model()` via [`set_allowed_tool_names`].
    /// When `Some`, only these tools can be executed; all others get `NotFound`.
    /// When `None` (default), all registered tools are executable.
    pub(crate) allowed_tool_names: Arc<std::sync::RwLock<Option<HashSet<String>>>>,
    /// Skill-level tool restriction.
    ///
    /// When a skill with `allowed_tools` is invoked, this is set to restrict
    /// which tools can be used during the skill execution. Applied as an
    /// intersection with `allowed_tool_names`.
    pub(crate) skill_allowed_tools: Arc<std::sync::RwLock<Option<HashSet<String>>>>,
    /// Optional OTel manager for metrics and traces.
    pub(crate) otel_manager: Option<Arc<cocode_otel::OtelManager>>,
    /// Session-level paths (session_dir, cocode_home, auto_memory_dir, plan_file_path).
    pub(crate) paths: crate::context::SessionPaths,
}

/// Result of pre-hook execution.
///
/// Carries the (possibly modified) input along with signals collected
/// from hook outcomes that the caller must act on.
pub(crate) struct PreHookOutcome {
    /// The (possibly modified) input.
    pub input: Value,
    /// If true, a hook returned PermissionOverride "allow" -- skip permission checks.
    pub skip_permission: bool,
    /// Additional contexts collected from `ContinueWithContext` hooks.
    pub additional_contexts: Vec<String>,
}

/// Permission level for hook-based permission aggregation.
///
/// Most-restrictive-wins: deny(0) > ask(1) > allow(2) > undefined(3).
/// Lower ordinal = more restrictive.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub(crate) enum PermissionLevel {
    Deny = 0,
    Ask = 1,
    Allow = 2,
    Undefined = 3,
}

impl PermissionLevel {
    pub fn from_decision(s: &str) -> Self {
        match s {
            "deny" => Self::Deny,
            "ask" => Self::Ask,
            "allow" => Self::Allow,
            _ => Self::Undefined,
        }
    }
}

/// Aggregates permission overrides from hook outcomes.
///
/// Uses most-restrictive-wins: deny > ask > allow > undefined.
/// Returns the aggregated permission level and the first deny reason (if any).
///
/// This function extracts the aggregation logic from `execute_pre_hooks()` for
/// unit testing. The production code uses the same logic inline (interleaved
/// with other outcome processing).
#[cfg(test)]
fn aggregate_permission_overrides(
    outcomes: &[cocode_hooks::HookOutcome],
) -> (PermissionLevel, Option<String>) {
    let mut aggregated = PermissionLevel::Undefined;
    let mut first_deny_reason: Option<String> = None;

    for outcome in outcomes {
        if let HookResult::PermissionOverride {
            ref decision,
            ref reason,
        } = outcome.result
        {
            let level = PermissionLevel::from_decision(decision);
            if level < aggregated {
                aggregated = level;
                if level == PermissionLevel::Deny && first_deny_reason.is_none() {
                    first_deny_reason = reason.clone();
                }
            }
        }
    }

    (aggregated, first_deny_reason)
}

impl StreamingToolExecutor {
    /// Set the allowlist of tool names that the model was given.
    ///
    /// Called from the driver after `select_tools_for_model()` resolves the
    /// final set of definitions. Any tool call whose name is not in this set
    /// is rejected with `NotFound`, preventing hallucinated or injected calls
    /// to tools the model was never offered (e.g. `apply_patch` when
    /// `apply_patch_tool_type` is `None`, or tools outside
    /// `experimental_supported_tools`).
    pub fn set_allowed_tool_names(&self, names: HashSet<String>) {
        *self
            .allowed_tool_names
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(names);

        // Generate a fresh batch ID for this streaming session's parallel tools
        *self
            .current_batch_id
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(uuid::Uuid::new_v4().to_string());

        // Reset sibling abort state for the new batch
        *self
            .sibling_abort_token
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = CancellationToken::new();
        *self
            .sibling_error_desc
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    /// Set skill-level tool restrictions.
    ///
    /// When a skill specifies `allowed_tools`, only those tools (plus "Skill")
    /// are allowed during the skill's execution.
    pub fn set_skill_allowed_tools(&self, tools: Option<HashSet<String>>) {
        *self
            .skill_allowed_tools
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = tools;
    }

    /// Check if a tool name is allowed by both the model allowlist and skill restrictions.
    ///
    /// Returns `true` only if the tool passes both checks:
    /// 1. Model allowlist: no allowlist set (all tools allowed) or the name is in the set
    /// 2. Skill restriction: no restriction set or the name is in the skill's allowed set
    fn is_tool_allowed(&self, name: &str) -> bool {
        // Check model-level allowlist
        let model_allowed = match self
            .allowed_tool_names
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            None => true,
            Some(set) => set.contains(name),
        };
        if !model_allowed {
            return false;
        }

        // Check skill-level restriction
        match self
            .skill_allowed_tools
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            None => true,
            Some(set) => set.contains(name),
        }
    }

    /// Get the async hook tracker for collecting completed async hooks.
    ///
    /// Call `tracker.take_completed()` to retrieve and clear completed hooks
    /// for injection into system reminders.
    ///
    /// ## Usage with System Reminders
    ///
    /// After each turn, collect completed async hooks and pass them to the
    /// system reminder generator context:
    ///
    /// ```ignore
    /// use cocode_system_reminder::{
    ///     AsyncHookResponseInfo, HookState, GeneratorContextBuilder,
    /// };
    ///
    /// // Collect completed hooks
    /// let completed = executor.async_hook_tracker().take_completed();
    ///
    /// // Convert to system reminder format
    /// let responses: Vec<AsyncHookResponseInfo> = completed
    ///     .into_iter()
    ///     .map(|h| AsyncHookResponseInfo {
    ///         hook_name: h.hook_name,
    ///         additional_context: h.additional_context,
    ///         was_blocking: h.was_blocking,
    ///         blocking_reason: h.blocking_reason,
    ///         duration_ms: h.duration_ms,
    ///     })
    ///     .collect();
    ///
    /// // Pass to generator context via typed HookState
    /// let ctx = GeneratorContextBuilder::new(&config)
    ///     .hook_state(HookState { async_responses: responses, ..Default::default() })
    ///     .build();
    /// ```
    pub fn async_hook_tracker(&self) -> &Arc<AsyncHookTracker> {
        &self.async_hook_tracker
    }

    /// Called when a tool_use block completes during streaming.
    ///
    /// For safe tools, execution starts immediately.
    /// For unsafe tools, they are queued for later execution.
    pub async fn on_tool_complete(&self, tool_call: ToolCall) {
        let call_id = &tool_call.tool_call_id;
        let name = &tool_call.tool_name;

        debug!(call_id = %call_id, name = %name, "Tool use complete");

        // Reject tools not in the model's allowlist (if set).
        // This prevents hallucinated calls to tools the model was never offered
        // (e.g., apply_patch when apply_patch_tool_type is None, or tools outside
        // experimental_supported_tools).
        if !self.is_tool_allowed(name) {
            debug!(call_id = %call_id, name = %name, "Tool not in allowed set, rejecting");
            let result =
                Err(crate::error::tool_error::NotFoundSnafu { name: name.clone() }.build());
            self.emit_completed(call_id, &result).await;
            self.executor_state
                .lock()
                .await
                .completed_results
                .push(ToolExecutionResult {
                    call_id: call_id.clone(),
                    name: name.clone(),
                    result,
                    additional_contexts: Vec::new(),
                    stop_continuation: None,
                });
            return;
        }

        // Emit queued event
        self.emit_stream(StreamEvent::ToolUseQueued {
            call_id: call_id.clone(),
            name: name.clone(),
            input: tool_call.input.clone(),
        })
        .await;

        // Check if tool exists and get concurrency safety
        let tool = match self.registry.get(name) {
            Some(t) => t,
            None => {
                // Queue for later - might be MCP tool
                self.executor_state
                    .lock()
                    .await
                    .pending_unsafe
                    .push(PendingToolCall { tool_call });
                return;
            }
        };

        let is_safe = tool.is_concurrency_safe_for(&tool_call.input);
        debug!(
            tool = %name,
            call_id = %call_id,
            is_safe,
            "Tool concurrency classification"
        );

        match is_safe {
            true => {
                // Check concurrency limit — single lock acquisition to avoid TOCTOU
                {
                    let mut lock = self.executor_state.lock().await;
                    if lock.active_tasks.len() >= self.config.max_concurrency as usize {
                        lock.pending_unsafe.push(PendingToolCall { tool_call });
                        return;
                    }
                }

                // Start immediately
                self.start_tool_execution(tool_call).await;
            }
            false => {
                // Queue for sequential execution
                self.executor_state
                    .lock()
                    .await
                    .pending_unsafe
                    .push(PendingToolCall { tool_call });
            }
        }
    }

    /// Run pre-hooks, apply permission overrides, and emit the started event.
    ///
    /// Returns the (possibly modified) input and any additional contexts from
    /// pre-hooks on success, or `None` if a pre-hook rejected the tool call
    /// (error result is already stored).
    async fn prepare_execution(
        &self,
        call_id: &str,
        name: &str,
        original_input: Value,
    ) -> Option<(Value, Vec<String>)> {
        let pre_hook = match self.execute_pre_hooks(name, call_id, original_input).await {
            Ok(outcome) => outcome,
            Err(reason) => {
                let result = Err(crate::error::tool_error::HookRejectedSnafu { reason }.build());
                self.emit_completed(call_id, &result).await;
                self.executor_state
                    .lock()
                    .await
                    .completed_results
                    .push(ToolExecutionResult {
                        call_id: call_id.to_string(),
                        name: name.to_string(),
                        result,
                        additional_contexts: Vec::new(),
                        stop_continuation: None,
                    });
                return None;
            }
        };

        if pre_hook.skip_permission {
            // Security guard: tools that require interactive confirmation (e.g.
            // ExitPlanMode) must still go through the permission pipeline even
            // if a hook pre-approved them.
            let tool_requires_interaction = self
                .registry
                .get(name)
                .is_some_and(|t| t.requires_user_interaction());

            if !tool_requires_interaction {
                let pattern = format!("hook-override-{call_id}");
                self.state
                    .approval_store
                    .lock()
                    .await
                    .approve_pattern(name, &pattern);
            }
        }

        for ctx_str in &pre_hook.additional_contexts {
            debug!(tool = %name, "Pre-hook additional context: {ctx_str}");
        }

        let batch_id = self
            .current_batch_id
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        self.emit_stream(StreamEvent::ToolUseStarted {
            call_id: call_id.to_string(),
            name: name.to_string(),
            batch_id,
        })
        .await;

        Some((pre_hook.input, pre_hook.additional_contexts))
    }

    /// Start tool execution in a background task.
    async fn start_tool_execution(&self, tool_call: ToolCall) {
        let call_id = tool_call.tool_call_id.clone();
        let name = tool_call.tool_name.clone();

        let (modified_input, pre_hook_contexts) = match self
            .prepare_execution(&call_id, &name, tool_call.input.clone())
            .await
        {
            Some(pair) => pair,
            None => return,
        };

        let ctx = self.create_context(&call_id);
        let registry = self.registry.clone();
        let timeout_secs = self.config.default_timeout_secs;
        let modified_tool_call = ToolCall::new(&call_id, &name, modified_input.clone());
        let hooks = self.hooks.clone();
        let session_id = self.config.session_id.clone();
        let cwd = self.config.cwd.clone();
        let session_dir = self.config.session_dir.clone();
        let tool_config = self.config.tool_config.clone();
        let max_tool_output_chars = self.config.max_tool_output_chars;
        let otel_manager = self.otel_manager.clone();
        let sibling_abort_token = self
            .sibling_abort_token
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let sibling_error_desc = self.sibling_error_desc.clone();
        let task_name = name.clone();

        let handle = tokio::spawn(async move {
            // Check sibling abort before starting
            if sibling_abort_token.is_cancelled() {
                let desc = sibling_error_desc
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
                    .unwrap_or_default();
                return ToolExecutionResult {
                    call_id,
                    name,
                    result: Ok(ToolOutput::error(format!(
                        "Cancelled: parallel tool call {desc} errored"
                    ))),
                    additional_contexts: Vec::new(),
                    stop_continuation: None,
                };
            }
            let tool_start = std::time::Instant::now();
            let result = execute_tool(
                &registry,
                modified_tool_call,
                ctx,
                timeout_secs,
                max_tool_output_chars,
                session_dir.as_deref(),
                &tool_config,
                otel_manager.as_ref(),
            )
            .await;

            let (post_action, post_hook_contexts) = run_post_hooks(
                hooks.as_deref(),
                &name,
                &call_id,
                &modified_input,
                &result,
                &session_id,
                &cwd,
            )
            .await;

            // Extract stop_continuation before passing post_action to finalize.
            let stop_continuation = match &post_action {
                PostHookAction::StopContinuation(reason) => Some(reason.clone()),
                _ => None,
            };

            let result = finalize_tool_result(
                result,
                post_action,
                &name,
                &call_id,
                tool_start,
                &otel_manager,
            );

            // Merge pre-hook and post-hook additional contexts.
            let mut additional_contexts = pre_hook_contexts;
            additional_contexts.extend(post_hook_contexts);

            // Sibling abort: if a Bash tool failed, cancel all parallel siblings.
            // Matches Claude Code's siblingAbortController pattern where Bash
            // is_error triggers abort("sibling_error") for all concurrent tools.
            if task_name == cocode_protocol::ToolName::Bash.as_str() {
                let is_error = match &result {
                    Ok(output) => output.is_error,
                    Err(_) => true,
                };
                if is_error {
                    // Set description first, then release the lock before
                    // cancelling to avoid holding it during cancel propagation.
                    {
                        let mut guard = sibling_error_desc
                            .write()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        *guard = Some(task_name.clone());
                    }
                    sibling_abort_token.cancel();
                }
            }

            ToolExecutionResult {
                call_id,
                name,
                result,
                additional_contexts,
                stop_continuation,
            }
        });

        self.executor_state
            .lock()
            .await
            .active_tasks
            .insert(tool_call.tool_call_id, handle);
    }

    /// Execute queued pending tools with dynamic scheduling.
    ///
    /// Uses CC v2.1.7-style dynamic queue processing:
    /// - Safe tools are spawned concurrently (up to `max_concurrency`)
    /// - When an unsafe tool is encountered, all active tasks are awaited first
    /// - After each completion, the next batch is evaluated
    pub async fn execute_pending_unsafe(&self) {
        let pending = {
            let mut lock = self.executor_state.lock().await;
            std::mem::take(&mut lock.pending_unsafe)
        };

        debug!(
            pending_count = pending.len(),
            "Executing pending unsafe tools"
        );

        let mut queue = std::collections::VecDeque::from(pending);

        while let Some(pending_call) = queue.pop_front() {
            if self.cancel_token.is_cancelled() {
                break;
            }

            let tool_call = pending_call.tool_call;
            let call_id = tool_call.tool_call_id.clone();
            let name = tool_call.tool_name.clone();

            // Reject tools not in the model's allowlist (if set)
            if !self.is_tool_allowed(&name) {
                debug!(call_id = %call_id, name = %name, "Tool not in allowed set, rejecting");
                let result =
                    Err(crate::error::tool_error::NotFoundSnafu { name: name.clone() }.build());
                self.emit_completed(&call_id, &result).await;
                self.executor_state
                    .lock()
                    .await
                    .completed_results
                    .push(ToolExecutionResult {
                        call_id,
                        name,
                        result,
                        additional_contexts: Vec::new(),
                        stop_continuation: None,
                    });
                continue;
            }

            // Check per-input concurrency safety
            let is_safe = self
                .registry
                .get(&name)
                .map(|tool| tool.is_concurrency_safe_for(&tool_call.input))
                .unwrap_or(false);

            if is_safe {
                // Safe tool: spawn concurrently (respecting max_concurrency)
                let active_count = self.executor_state.lock().await.active_tasks.len();
                if active_count >= self.config.max_concurrency as usize {
                    // Wait for at least one active task to complete before spawning more
                    self.drain_one_active().await;
                }
                self.start_tool_execution(tool_call).await;
            } else {
                // Unsafe tool: drain all active tasks first, then execute sequentially
                self.drain_active_tasks().await;
                self.execute_single_tool(tool_call).await;
            }
        }

        // Drain any remaining active tasks spawned during the loop
        self.drain_active_tasks().await;
    }

    /// Execute a single tool synchronously (for unsafe tools in the pending queue).
    async fn execute_single_tool(&self, tool_call: ToolCall) {
        let call_id = tool_call.tool_call_id.clone();
        let name = tool_call.tool_name.clone();

        let (modified_input, pre_hook_contexts) = match self
            .prepare_execution(&call_id, &name, tool_call.input.clone())
            .await
        {
            Some(pair) => pair,
            None => return,
        };

        let tool_start = std::time::Instant::now();
        let ctx = self.create_context(&call_id);
        let modified_tool_call = ToolCall::new(&call_id, &name, modified_input.clone());
        let result = execute_tool(
            &self.registry,
            modified_tool_call,
            ctx,
            self.config.default_timeout_secs,
            self.config.max_tool_output_chars,
            self.config.session_dir.as_deref(),
            &self.config.tool_config,
            self.otel_manager.as_ref(),
        )
        .await;

        let (post_action, post_hook_contexts) = self
            .execute_post_hooks(&name, &call_id, &modified_input, &result)
            .await;

        // Extract stop_continuation before passing post_action to finalize.
        let stop_continuation = match &post_action {
            PostHookAction::StopContinuation(reason) => Some(reason.clone()),
            _ => None,
        };

        let result = finalize_tool_result(
            result,
            post_action,
            &name,
            &call_id,
            tool_start,
            &self.otel_manager,
        );

        // Merge pre-hook and post-hook additional contexts.
        let mut additional_contexts = pre_hook_contexts;
        additional_contexts.extend(post_hook_contexts);

        self.emit_completed(&call_id, &result).await;
        self.executor_state
            .lock()
            .await
            .completed_results
            .push(ToolExecutionResult {
                call_id,
                name,
                result,
                additional_contexts,
                stop_continuation,
            });
    }

    /// Wait for all active tasks to complete and collect their results.
    ///
    /// If sibling abort has been triggered (e.g. Bash error), tasks that were
    /// aborted will produce synthetic error results matching Claude Code's
    /// `<tool_use_error>Cancelled: parallel tool call ... errored</tool_use_error>`.
    async fn drain_active_tasks(&self) {
        let tasks: Vec<_> = {
            let mut lock = self.executor_state.lock().await;
            lock.active_tasks.drain().collect()
        };

        let mut collected = Vec::with_capacity(tasks.len());
        for (call_id, handle) in tasks {
            match handle.await {
                Ok(result) => {
                    self.emit_completed(&result.call_id, &result.result).await;
                    collected.push(result);
                }
                Err(e) if e.is_cancelled() => {
                    let desc = self
                        .sibling_error_desc
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    let msg = match desc {
                        Some(d) => format!("Cancelled: parallel tool call {d} errored"),
                        None => "Cancelled: parallel tool call errored".to_string(),
                    };
                    let result = Ok(ToolOutput::error(msg));
                    self.emit_completed(&call_id, &result).await;
                    collected.push(ToolExecutionResult {
                        call_id: call_id.clone(),
                        name: format!("<aborted:{call_id}>"),
                        result,
                        additional_contexts: Vec::new(),
                        stop_continuation: None,
                    });
                }
                Err(e) => {
                    error!(call_id = %call_id, error = %e, "Task panicked");
                    let result = Err(crate::error::tool_error::InternalSnafu {
                        message: format!("Tool execution task panicked (call_id: {call_id}): {e}"),
                    }
                    .build());
                    self.emit_completed(&call_id, &result).await;
                    collected.push(ToolExecutionResult {
                        call_id: call_id.clone(),
                        name: format!("<panicked:{call_id}>"),
                        result,
                        additional_contexts: Vec::new(),
                        stop_continuation: None,
                    });
                }
            }
        }

        // Single lock acquisition for all results
        self.executor_state
            .lock()
            .await
            .completed_results
            .extend(collected);
    }

    /// Wait for whichever active task finishes first and collect its result.
    ///
    /// Uses `select_all` to race all active handles so we don't block on an
    /// arbitrary slow task while faster ones are ready.
    async fn drain_one_active(&self) {
        let entries: Vec<(String, JoinHandle<ToolExecutionResult>)> = {
            let mut lock = self.executor_state.lock().await;
            lock.active_tasks.drain().collect()
        };

        if entries.is_empty() {
            return;
        }

        let (ids, handles): (Vec<_>, Vec<_>) = entries.into_iter().unzip();

        let (join_result, winner_idx, remaining) = futures::future::select_all(handles).await;

        // Re-insert the handles that didn't finish yet.
        // `remaining` is in original order with the winner removed.
        {
            let mut lock = self.executor_state.lock().await;
            let mut orig_idx = 0;
            for handle in remaining {
                if orig_idx == winner_idx {
                    orig_idx += 1;
                }
                lock.active_tasks.insert(ids[orig_idx].clone(), handle);
                orig_idx += 1;
            }
        }

        let call_id = &ids[winner_idx];
        match join_result {
            Ok(result) => {
                self.emit_completed(&result.call_id, &result.result).await;
                self.executor_state
                    .lock()
                    .await
                    .completed_results
                    .push(result);
            }
            Err(e) => {
                error!(call_id = %call_id, error = %e, "Task panicked");
                let result = Err(crate::error::tool_error::InternalSnafu {
                    message: format!("Tool execution task panicked (call_id: {call_id}): {e}"),
                }
                .build());
                self.emit_completed(call_id, &result).await;
                self.executor_state
                    .lock()
                    .await
                    .completed_results
                    .push(ToolExecutionResult {
                        call_id: call_id.clone(),
                        name: format!("<panicked:{call_id}>"),
                        result,
                        additional_contexts: Vec::new(),
                        stop_continuation: None,
                    });
            }
        }
    }

    /// Wait for all active tasks and return their results.
    pub async fn drain(&self) -> Vec<ToolExecutionResult> {
        self.drain_active_tasks().await;

        // Return all completed results
        let mut lock = self.executor_state.lock().await;
        std::mem::take(&mut lock.completed_results)
    }

    /// Abort a running tool by call ID.
    pub async fn abort(&self, call_id: &str, reason: AbortReason) {
        // Cancel the token associated with this tool
        // Note: In a full implementation, each tool would have its own cancel token
        info!(call_id = %call_id, reason = ?reason, "Aborting tool");

        // Remove from active tasks
        if let Some(handle) = self
            .executor_state
            .lock()
            .await
            .active_tasks
            .remove(call_id)
        {
            handle.abort();
        }

        // Emit aborted event
        self.emit_tui(TuiEvent::ToolExecutionAborted { reason })
            .await;
    }

    /// Abort all running and pending tools.
    pub async fn abort_all(&self, reason: AbortReason) {
        // Cancel all active tasks and clear pending in a single lock acquisition
        let tasks: Vec<_> = {
            let mut lock = self.executor_state.lock().await;
            lock.pending_unsafe.clear();
            lock.active_tasks.drain().collect()
        };

        for (_, handle) in tasks {
            handle.abort();
        }

        // Emit aborted event
        self.emit_tui(TuiEvent::ToolExecutionAborted { reason })
            .await;
    }

    /// Get the number of active tasks.
    pub async fn active_count(&self) -> usize {
        self.executor_state.lock().await.active_tasks.len()
    }

    /// Get the number of pending unsafe tasks.
    pub async fn pending_count(&self) -> usize {
        self.executor_state.lock().await.pending_unsafe.len()
    }

    /// Set a shared invoked skills tracker.
    ///
    /// The driver passes its own Arc so invoked skills persist across turns.
    pub fn set_invoked_skills(&mut self, skills: Arc<Mutex<Vec<crate::context::InvokedSkill>>>) {
        self.state.invoked_skills = skills;
    }

    /// Get the shared invoked skills tracker.
    ///
    /// Returns the Arc to the invoked skills list. After tool execution,
    /// the driver can read this to inject invoked skills into system reminders.
    pub fn invoked_skills(&self) -> &Arc<Mutex<Vec<crate::context::InvokedSkill>>> {
        &self.state.invoked_skills
    }

    /// Get the shared agent cancel token registry.
    ///
    /// The driver / subagent manager should register cancel tokens here
    /// when spawning agents, so TaskStop can cancel them by ID.
    pub fn agent_cancel_tokens(&self) -> &crate::context::AgentCancelTokens {
        &self.agent.agent_cancel_tokens
    }

    /// Get the shared killed agents registry.
    pub fn killed_agents(&self) -> &crate::context::KilledAgents {
        &self.agent.killed_agents
    }

    /// Create a tool context for execution.
    fn create_context(&self, call_id: &str) -> ToolContext {
        // Build identity
        let identity = crate::context::ToolCallIdentity {
            call_id: call_id.to_string(),
            session_id: self.config.session_id.clone(),
            turn_id: self.config.turn_id.clone(),
            turn_number: self.config.turn_number,
            agent_id: None,
        };

        // Build environment — cwd comes from the shell executor (live CWD tracking)
        let env = ToolEnvironment {
            cwd: self.services.shell_executor.cwd(),
            additional_working_directories: Vec::new(),
            permission_mode: self.config.permission_mode,
            features: self.config.features.clone(),
            web_search_config: self.config.web_search_config.clone(),
            web_fetch_config: self.config.web_fetch_config.clone(),
            task_type_restrictions: self.config.task_type_restrictions.clone(),
            is_plan_mode: self.config.is_plan_mode,
            is_ultraplan: self.config.is_ultraplan,
        };

        let channels = ToolChannels {
            event_tx: self.event_tx.clone(),
            cancel_token: self.cancel_token.clone(),
        };

        let mut agent = self.agent.clone();
        agent.team_name = self.config.team_name.clone();

        ToolContext {
            identity,
            env,
            channels,
            state: self.state.clone(),
            services: self.services.clone(),
            agent,
            paths: self.paths.clone(),
        }
    }

    pub(crate) async fn emit_protocol(&self, notif: ServerNotification) {
        if let Some(tx) = &self.event_tx
            && let Err(e) = tx.send(CoreEvent::Protocol(notif)).await
        {
            debug!("Failed to send protocol event: {e}");
        }
    }

    pub(crate) async fn emit_stream(&self, event: StreamEvent) {
        if let Some(tx) = &self.event_tx
            && let Err(e) = tx.send(CoreEvent::Stream(event)).await
        {
            debug!("Failed to send stream event: {e}");
        }
    }

    async fn emit_tui(&self, event: TuiEvent) {
        if let Some(tx) = &self.event_tx
            && let Err(e) = tx.send(CoreEvent::Tui(event)).await
        {
            debug!("Failed to send TUI event: {e}");
        }
    }

    /// Emit a completed event.
    pub(crate) async fn emit_completed(&self, call_id: &str, result: &Result<ToolOutput>) {
        let (output, is_error) = match result {
            Ok(output) => (output.content.clone(), output.is_error),
            Err(e) => (
                cocode_protocol::ToolResultContent::Text(e.to_string()),
                true,
            ),
        };

        self.emit_stream(StreamEvent::ToolUseCompleted {
            call_id: call_id.to_string(),
            output,
            is_error,
        })
        .await;
    }
}

/// Apply a post-hook action and record OTel metrics.
fn finalize_tool_result(
    result: Result<ToolOutput>,
    post_action: PostHookAction,
    name: &str,
    call_id: &str,
    start: std::time::Instant,
    otel_manager: &Option<Arc<cocode_otel::OtelManager>>,
) -> Result<ToolOutput> {
    let result = match post_action {
        PostHookAction::None | PostHookAction::StopContinuation(_) => result,
        PostHookAction::Reject(reason) => Ok(ToolOutput::error(format!(
            "PostToolUse hook blocked this tool's output: {reason}"
        ))),
        PostHookAction::ReplaceOutput(output) => Ok(output),
    };

    if let Some(otel) = otel_manager {
        otel.tool_result(name, call_id, "", start.elapsed(), result.is_ok(), "");
    }

    result
}

/// Execute a single tool with timeout and cancellation support.
#[allow(clippy::too_many_arguments)]
async fn execute_tool(
    registry: &ToolRegistry,
    tool_call: ToolCall,
    mut ctx: ToolContext,
    timeout_secs: i64,
    max_tool_output_chars: Option<i32>,
    session_dir: Option<&Path>,
    tool_config: &cocode_protocol::ToolConfig,
    otel_manager: Option<&Arc<cocode_otel::OtelManager>>,
) -> Result<ToolOutput> {
    let timeout_duration = std::time::Duration::from_secs(timeout_secs as u64);
    let cancel_token = ctx.channels.cancel_token.clone();

    tokio::select! {
        biased;
        _ = cancel_token.cancelled() => {
            Err(crate::error::tool_error::CancelledSnafu.build())
        }
        result = tokio::time::timeout(
            timeout_duration,
            execute_tool_inner(
                registry,
                tool_call,
                &mut ctx,
                max_tool_output_chars,
                session_dir,
                tool_config,
                otel_manager,
            ),
        ) => {
            match result {
                Ok(inner) => inner,
                Err(_) => Err(crate::error::tool_error::TimeoutSnafu { timeout_secs }.build()),
            }
        }
    }
}

/// Check if a tool name is an edit/write tool (for AcceptEdits mode).
///
/// Delegates to the tool's own [`Tool::is_edit_tool`] declaration via registry lookup.
fn is_edit_tool(registry: &ToolRegistry, name: &str) -> bool {
    registry.get(name).is_some_and(|t| t.is_edit_tool())
}

/// Check if a tool name is read-only or a plan mode control tool.
///
/// Plan control tools are always allowed regardless of the tool's own declaration.
/// MCP tools (prefixed with `mcp__`) always bypass plan mode filtering, matching
/// Claude Code's behavior where MCP tools are always available in plan mode.
/// For all other tools, delegates to the tool's [`Tool::is_read_only`] via registry lookup.
fn is_read_only_or_plan_tool(registry: &ToolRegistry, name: &str) -> bool {
    use cocode_protocol::ToolName;

    // MCP tools always bypass plan mode filtering (CC: mcp__ prefix check in Xk8)
    if name.starts_with(cocode_protocol::MCP_TOOL_PREFIX) {
        return true;
    }

    // Plan control tools (always allowed in plan mode)
    const PLAN_CONTROL: &[&str] = &[
        ToolName::EnterPlanMode.as_str(),
        ToolName::ExitPlanMode.as_str(),
        ToolName::AskUserQuestion.as_str(),
        ToolName::TodoWrite.as_str(),
        ToolName::TaskCreate.as_str(),
        ToolName::TaskUpdate.as_str(),
    ];
    if PLAN_CONTROL.contains(&name) {
        return true;
    }
    // Delegate to tool's own declaration
    registry.get(name).is_some_and(|t| t.is_read_only())
}

/// Extract file_path from tool input if present.
fn extract_file_path(input: &Value) -> Option<std::path::PathBuf> {
    input
        .get("file_path")
        .or_else(|| input.get("notebook_path"))
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from)
}

/// Extract the raw value used for approval pattern matching.
///
/// For Bash tools, this is the raw command string (e.g., "git push origin main").
/// For file tools, this is the file path.
/// For other tools, falls back to the description.
///
/// This is separate from `ApprovalRequest.description` (which includes the tool
/// name prefix like "Bash: git push origin main") so that wildcard patterns
/// like "git *" match correctly against the raw command, not the prefixed description.
fn approval_check_value(name: &str, input: &Value, description: &str) -> String {
    // For shell tools, use the raw command
    if let Some(cmd) = extract_command_input(name, input) {
        return cmd;
    }
    // For file tools, use the path
    if let Some(path) = extract_file_path(input) {
        return path.display().to_string();
    }
    // Fallback to description
    description.to_string()
}

/// Extract a command prefix pattern for the "allow similar commands" option.
///
/// For Bash commands, extracts the first word as a prefix pattern.
/// E.g. `"git push origin main"` -> `Some("git *")`.
fn extract_prefix_pattern(tool_name: &str, input: &Value) -> Option<String> {
    if tool_name != cocode_protocol::ToolName::Bash.as_str() {
        return None;
    }
    let command = input.get("command").and_then(|v| v.as_str())?;
    let first_word = command.split_whitespace().next()?;
    if first_word.is_empty() {
        return None;
    }
    Some(format!("{first_word} *"))
}

/// Build a default approval request for a tool that needs user approval.
fn default_approval_request(
    name: &str,
    input: &Value,
    agent_id: Option<&str>,
) -> cocode_protocol::ApprovalRequest {
    let description = if let Some(path) = extract_file_path(input) {
        format!("{name}: {}", path.display())
    } else if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
        let truncated = if cmd.len() > 80 {
            format!("{}...", &cmd[..80])
        } else {
            cmd.to_string()
        };
        format!("{name}: {truncated}")
    } else {
        format!("Execute tool: {name}")
    };

    let proposed_prefix_pattern = extract_prefix_pattern(name, input);

    cocode_protocol::ApprovalRequest {
        request_id: format!("default-{name}-{}", uuid::Uuid::new_v4()),
        tool_name: name.to_string(),
        description,
        risks: vec![],
        allow_remember: true,
        proposed_prefix_pattern,
        input: Some(input.clone()),
        source_agent_id: agent_id.map(String::from),
    }
}

/// Extract command string from shell tool input.
fn extract_command_input(name: &str, input: &Value) -> Option<String> {
    use cocode_protocol::ToolName;
    match name {
        n if n == ToolName::Bash.as_str() => input
            .get("command")
            .and_then(|v| v.as_str())
            .map(String::from),
        n if n == ToolName::Shell.as_str() => {
            input.get("command").and_then(|v| v.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
        }
        _ => None,
    }
}

/// Full permission pipeline (5 stages) aligned with Claude Code v2.1.7.
///
/// 1. Check DENY rules (all sources) -> if match -> Deny
/// 2. Check ASK rules (all sources) -> if match -> NeedsApproval
/// 3. Tool-specific check_permission() -> returns allow/deny/ask/passthrough
/// 4. Check ALLOW rules (all sources) -> if match -> Allow
/// 5. Default behavior: reads -> Allow, writes -> NeedsApproval
async fn check_permission_pipeline(
    tool: &dyn crate::tool::Tool,
    name: &str,
    input: &Value,
    ctx: &ToolContext,
) -> cocode_protocol::PermissionResult {
    let file_path = extract_file_path(input);
    let command_input = extract_command_input(name, input);

    if let Some(ref evaluator) = ctx.services.permission_evaluator {
        // Stages 1+2: Check DENY then ASK rules
        if let Some(decision) =
            evaluator.evaluate_deny_ask(name, file_path.as_deref(), command_input.as_deref())
        {
            if decision.result.is_denied() {
                return decision.result;
            }
            // ASK rule matched -- the tool must ask for approval
            return cocode_protocol::PermissionResult::NeedsApproval {
                request: cocode_protocol::ApprovalRequest {
                    request_id: format!("rule-ask-{name}"),
                    tool_name: name.to_string(),
                    description: decision.reason,
                    risks: vec![],
                    allow_remember: true,
                    proposed_prefix_pattern: extract_prefix_pattern(name, input),
                    input: Some(input.clone()),
                    source_agent_id: ctx.identity.agent_id.clone(),
                },
            };
        }
    }

    // Stage 3: Tool-specific check
    let tool_result = tool.check_permission(input, ctx).await;
    if !tool_result.is_passthrough() {
        return tool_result;
    }

    if let Some(ref evaluator) = ctx.services.permission_evaluator {
        // Stage 4: Check ALLOW rules
        if let Some(decision) = evaluator.evaluate_behavior(
            name,
            file_path.as_deref(),
            cocode_policy::RuleAction::Allow,
            command_input.as_deref(),
        ) && decision.result.is_allowed()
        {
            return cocode_protocol::PermissionResult::Allowed;
        }
    }

    // Stage 5: Default behavior
    if tool.is_read_only() {
        cocode_protocol::PermissionResult::Allowed
    } else {
        cocode_protocol::PermissionResult::NeedsApproval {
            request: default_approval_request(name, input, ctx.identity.agent_id.as_deref()),
        }
    }
}

/// Apply permission mode on top of pipeline result.
///
/// Converts results based on the current mode:
/// - Bypass: everything except Denied -> Allowed (deny is absolute)
/// - DontAsk: NeedsApproval -> Denied
/// - AcceptEdits: edit/write NeedsApproval -> Allowed
/// - Plan: non-read-only -> Denied
fn apply_permission_mode(
    result: cocode_protocol::PermissionResult,
    mode: PermissionMode,
    tool_name: &str,
    registry: &ToolRegistry,
) -> cocode_protocol::PermissionResult {
    match mode {
        // Bypass allows everything EXCEPT explicit denials -- deny is absolute.
        PermissionMode::Bypass => match result {
            cocode_protocol::PermissionResult::Denied { .. } => result,
            _ => cocode_protocol::PermissionResult::Allowed,
        },
        // Auto auto-approves NeedsApproval but respects explicit denials.
        // Currently identical to Bypass but kept separate for future differentiation.
        PermissionMode::Auto => match result {
            cocode_protocol::PermissionResult::Denied { .. } => result,
            _ => cocode_protocol::PermissionResult::Allowed,
        },
        PermissionMode::DontAsk => match result {
            cocode_protocol::PermissionResult::NeedsApproval { request } => {
                cocode_protocol::PermissionResult::Denied {
                    reason: format!(
                        "DontAsk mode: permission prompt suppressed for '{}': {}",
                        tool_name, request.description
                    ),
                }
            }
            other => other,
        },
        PermissionMode::AcceptEdits if is_edit_tool(registry, tool_name) => match result {
            cocode_protocol::PermissionResult::NeedsApproval { .. } => {
                cocode_protocol::PermissionResult::Allowed
            }
            other => other,
        },
        PermissionMode::Plan if !is_read_only_or_plan_tool(registry, tool_name) => {
            // In plan mode, deny non-read-only tools -- but respect tool-level Allowed
            // (e.g., plan file writes that the tool explicitly auto-allowed)
            match result {
                cocode_protocol::PermissionResult::Allowed => result,
                cocode_protocol::PermissionResult::NeedsApproval { .. } => {
                    cocode_protocol::PermissionResult::Denied {
                        reason: "Plan mode: only read-only tools allowed".to_string(),
                    }
                }
                other => other,
            }
        }
        _ => result,
    }
}

/// Inner tool execution logic (without timeout).
#[tracing::instrument(skip_all, fields(tool = %tool_call.tool_name, call_id = %tool_call.tool_call_id))]
async fn execute_tool_inner(
    registry: &ToolRegistry,
    tool_call: ToolCall,
    ctx: &mut ToolContext,
    max_tool_output_chars: Option<i32>,
    session_dir: Option<&Path>,
    tool_config: &cocode_protocol::ToolConfig,
    otel_manager: Option<&Arc<cocode_otel::OtelManager>>,
) -> Result<ToolOutput> {
    let call_id = &tool_call.tool_call_id;
    let name = &tool_call.tool_name;
    let input = tool_call.input;

    // Get the tool
    let tool = registry
        .get(name)
        .ok_or_else(|| crate::error::tool_error::NotFoundSnafu { name: name.clone() }.build())?;

    // Defense-in-depth: reject calls to feature-gated tools that are disabled.
    // Normally the model never sees these (definitions_filtered excludes them),
    // but a hallucinated or injected tool name could still reach here.
    if let Some(feature) = tool.feature_gate()
        && !ctx.env.features.enabled(feature)
    {
        return Err(crate::error::tool_error::NotFoundSnafu { name: name.clone() }.build());
    }

    // Validate input
    debug!(tool = %name, call_id = %call_id, "Stage 1: Validating tool input");
    let validation = tool.validate(&input).await;
    if let ValidationResult::Invalid { errors } = validation {
        let error_msgs: Vec<String> = errors
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        warn!(tool = %name, errors = ?error_msgs, "Stage 1: Validation failed");
        return Err(crate::error::tool_error::InvalidInputSnafu {
            message: error_msgs.join(", "),
        }
        .build());
    }

    // Run the full permission pipeline
    debug!(tool = %name, "Stage 2: Checking permissions");
    let pipeline_result = check_permission_pipeline(tool.as_ref(), name, &input, ctx).await;

    // Apply permission mode on top
    let permission =
        apply_permission_mode(pipeline_result, ctx.env.permission_mode, name, registry);

    match permission {
        cocode_protocol::PermissionResult::Allowed => {
            if let Some(otel) = otel_manager {
                otel.tool_decision(
                    name,
                    call_id,
                    "allowed",
                    cocode_otel::ToolDecisionSource::Config,
                );
            }
        }
        cocode_protocol::PermissionResult::Denied { reason } => {
            if let Some(otel) = otel_manager {
                otel.tool_decision(
                    name,
                    call_id,
                    "denied",
                    cocode_otel::ToolDecisionSource::Config,
                );
            }
            return Err(
                crate::error::tool_error::PermissionDeniedSnafu { message: reason }.build(),
            );
        }
        cocode_protocol::PermissionResult::NeedsApproval { request } => {
            // Use the raw command/path for approval matching, not the prefixed description.
            // This ensures wildcard patterns like "git *" match "git push origin main"
            // rather than failing against "Bash: git push origin main".
            let pattern = approval_check_value(name, &input, &request.description);
            if ctx.is_approved(name, &pattern).await {
                // Already approved for this pattern (exact or wildcard)
                if let Some(otel) = otel_manager {
                    otel.tool_decision(
                        name,
                        call_id,
                        "allowed",
                        cocode_otel::ToolDecisionSource::Config,
                    );
                }
            } else if let Some(hooks) = &ctx.services.hook_registry
                && let Some(override_decision) = {
                    // Fire PermissionRequest hooks before interactive approval
                    let perm_ctx = HookContext::new(
                        HookEventType::PermissionRequest,
                        ctx.identity.session_id.clone(),
                        ctx.env.cwd.clone(),
                    )
                    .with_tool(name, input.clone())
                    .with_tool_use_id(call_id)
                    .with_permission_mode(match ctx.env.permission_mode {
                        PermissionMode::Default => "default",
                        PermissionMode::Plan => "plan",
                        PermissionMode::AcceptEdits => "acceptEdits",
                        PermissionMode::Auto => "auto",
                        PermissionMode::Bypass => "bypassPermissions",
                        PermissionMode::DontAsk => "dontAsk",
                    });
                    let outcomes = hooks.execute(&perm_ctx).await;
                    let mut decision = None;
                    for outcome in outcomes {
                        if let HookResult::PermissionOverride {
                            decision: d,
                            reason,
                        } = outcome.result
                        {
                            info!(
                                hook = %outcome.hook_name,
                                decision = %d,
                                reason = ?reason,
                                "PermissionRequest hook overrode decision"
                            );
                            decision = Some((d, reason));
                            break;
                        }
                    }
                    decision
                }
            {
                // Hook provided a permission override
                match override_decision.0.as_str() {
                    "allow" => {
                        if let Some(otel) = otel_manager {
                            otel.tool_decision(
                                name,
                                call_id,
                                "allowed",
                                cocode_otel::ToolDecisionSource::Config,
                            );
                        }
                        // Auto-approve: remember in session
                        ctx.approve_pattern(name, &pattern).await;
                    }
                    "deny" => {
                        if let Some(otel) = otel_manager {
                            otel.tool_decision(
                                name,
                                call_id,
                                "denied",
                                cocode_otel::ToolDecisionSource::Config,
                            );
                        }
                        let reason = override_decision
                            .1
                            .unwrap_or_else(|| "PermissionRequest hook denied".to_string());
                        return Err(crate::error::tool_error::PermissionDeniedSnafu {
                            message: reason,
                        }
                        .build());
                    }
                    _ => {
                        // "ask" or unknown -- fall through to interactive approval
                    }
                }
            } else if let Some(requester) = &ctx.services.permission_requester {
                // Use the permission requester for interactive approval
                let worker_id = ctx.identity.call_id.clone();
                let decision = requester
                    .request_permission(request.clone(), &worker_id)
                    .await;
                match decision {
                    cocode_protocol::ApprovalDecision::Denied => {
                        if let Some(otel) = otel_manager {
                            otel.tool_decision(
                                name,
                                call_id,
                                "denied",
                                cocode_otel::ToolDecisionSource::User,
                            );
                        }
                        return Err(crate::error::tool_error::PermissionDeniedSnafu {
                            message: format!("User denied permission for tool '{name}'"),
                        }
                        .build());
                    }
                    cocode_protocol::ApprovalDecision::Approved => {
                        if let Some(otel) = otel_manager {
                            otel.tool_decision(
                                name,
                                call_id,
                                "approved",
                                cocode_otel::ToolDecisionSource::User,
                            );
                        }
                        // Session-only: remember exact value
                        ctx.approve_pattern(name, &pattern).await;
                    }
                    cocode_protocol::ApprovalDecision::ApprovedWithPrefix { prefix_pattern } => {
                        if let Some(otel) = otel_manager {
                            otel.tool_decision(
                                name,
                                call_id,
                                "approved",
                                cocode_otel::ToolDecisionSource::User,
                            );
                        }
                        // Remember prefix pattern in session + persist to disk
                        ctx.approve_pattern(name, &prefix_pattern).await;
                        ctx.persist_permission_rule(name, &prefix_pattern).await;
                    }
                }
            } else {
                // No permission requester available - deny
                if let Some(otel) = otel_manager {
                    otel.tool_decision(
                        name,
                        call_id,
                        "denied",
                        cocode_otel::ToolDecisionSource::Config,
                    );
                }
                return Err(crate::error::tool_error::PermissionDeniedSnafu {
                    message: format!("Tool '{name}' requires approval: {}", request.description),
                }
                .build());
            }
        }
        cocode_protocol::PermissionResult::Passthrough => {
            // Should not happen after pipeline -- treat as allowed
        }
    }

    // Pre-execute file backup (Tier 1 rewind)
    if !tool.is_read_only()
        && let Some(ref backup_store) = ctx.services.file_backup_store
        && let Some(file_path) = extract_file_path(&input)
        && let Err(e) = backup_store.backup_before_modify(&file_path).await
    {
        tracing::warn!("File backup failed for {}: {e}", file_path.display());
    }

    // Execute
    debug!(tool = %name, call_id = %call_id, "Stage 3: Executing tool");
    trace!(
        tool = %name,
        input = %cocode_utils_string::truncate_for_log(&input.to_string(), 512),
        "Stage 3: Tool input"
    );
    let execute_start = std::time::Instant::now();
    let result = tool.execute(input, ctx).await;
    let execute_duration_ms = execute_start.elapsed().as_millis() as i64;

    // Post-process
    let mut output = match result {
        Ok(output) => {
            debug!(
                tool = %name,
                duration_ms = execute_duration_ms,
                is_error = output.is_error,
                "Stage 3: Tool execution completed"
            );
            debug!(tool = %name, "Stage 4: Post-processing");
            tool.post_process(output, ctx).await
        }
        Err(e) => {
            warn!(
                tool = %name,
                duration_ms = execute_duration_ms,
                error = %e,
                "Stage 3: Tool execution failed"
            );
            return Err(e);
        }
    };

    // Persist oversized results BEFORE truncation.
    // Uses per-tool limit as the threshold so full output is saved to disk
    // when it exceeds the tool's normal output size.
    let per_tool_limit = tool.max_result_size_chars() as usize;
    if let Some(dir) = session_dir {
        output = result_persistence::persist_if_needed(
            output,
            call_id,
            dir,
            per_tool_limit,
            tool_config,
        )
        .await;
    }

    // Apply truncation: use the smaller of per-tool limit and model-level limit.
    // After persistence, the output is either:
    // - A small <persisted-output> block (if persisted) -> truncation is a no-op
    // - The original output <= per_tool_limit -> truncation only fires if model_limit is smaller
    let max_chars = match max_tool_output_chars {
        Some(model_limit) => per_tool_limit.min(model_limit as usize),
        None => per_tool_limit,
    };
    output.truncate_to(max_chars);
    if tracing::enabled!(tracing::Level::TRACE) {
        trace!(
            tool = %name,
            is_error = output.is_error,
            output = %cocode_utils_string::truncate_for_log(&format!("{:?}", output.content), 512),
            "Stage 4: Tool output (post-truncation)"
        );
    }

    // Cleanup
    trace!(tool = %name, "Stage 5: Cleanup");
    tool.cleanup(ctx).await;

    Ok(output)
}

impl std::fmt::Debug for StreamingToolExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamingToolExecutor")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "executor.test.rs"]
mod tests;
