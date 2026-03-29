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
use crate::context::FileTracker;
use crate::context::ModelCallFn;
use crate::context::SpawnAgentFn;
use crate::context::ToolContext;
use crate::context::ToolContextBuilder;
use crate::error::Result;
use crate::registry::ToolRegistry;
use crate::result_persistence;
use cocode_hooks::AsyncHookTracker;
use cocode_hooks::HookContext;
use cocode_hooks::HookEventType;
use cocode_hooks::HookRegistry;
use cocode_hooks::HookResult;
use cocode_policy::ApprovalStore;
use cocode_protocol::AbortReason;
use cocode_protocol::CoreEvent;
use cocode_protocol::PermissionMode;
use cocode_protocol::StreamEvent;
use cocode_protocol::ToolOutput;
use cocode_protocol::TuiEvent;
use cocode_protocol::ValidationResult;
use cocode_protocol::server_notification::*;
use cocode_shell::ShellExecutor;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
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

/// Default maximum concurrent tool executions.
pub const DEFAULT_MAX_TOOL_CONCURRENCY: i32 = 10;

/// Action determined by post-hook execution.
///
/// Distinguishes between a hook rejecting the output (error) and a hook
/// providing a replacement output (non-error substitution).
enum PostHookAction {
    /// No post-hook intervened — use the original result.
    None,
    /// A hook rejected the output — replace with an error.
    Reject(String),
    /// A hook provided replacement output (ModifyOutput) — use as-is.
    ReplaceOutput(ToolOutput),
    /// A hook requested that the agent loop stop after processing this tool result.
    /// The original output is preserved — only the loop continuation is affected.
    StopContinuation(String),
}

/// Configuration for the tool executor.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Maximum concurrent tool executions.
    ///
    /// Configurable via `COCODE_MAX_TOOL_USE_CONCURRENCY` environment variable.
    /// Default: 10.
    pub max_concurrency: i32,
    /// Working directory for tool execution.
    pub cwd: PathBuf,
    /// Session ID.
    pub session_id: String,
    /// Turn ID for the current turn.
    pub turn_id: String,
    /// Turn number for the current turn (1-indexed).
    pub turn_number: i32,
    /// Permission mode.
    pub permission_mode: PermissionMode,
    /// Default timeout for tool execution (seconds).
    pub default_timeout_secs: i64,
    /// Whether plan mode is currently active.
    pub is_plan_mode: bool,
    /// Whether this is an ultraplan session (plan pre-written by a remote session).
    pub is_ultraplan: bool,
    /// Path to the current plan file (if in plan mode).
    pub plan_file_path: Option<PathBuf>,
    /// Auto memory directory path (for write permission bypass).
    pub auto_memory_dir: Option<PathBuf>,
    /// Session directory for storing large tool results.
    ///
    /// When set, tool results exceeding the configured size threshold are
    /// persisted to `{session_dir}/tool-results/{call_id}.txt`.
    pub session_dir: Option<PathBuf>,
    /// Tool configuration for result persistence settings (preview size, enable flag).
    pub tool_config: cocode_protocol::ToolConfig,
    /// Feature flags for tool enablement.
    pub features: cocode_protocol::Features,
    /// Web search configuration.
    pub web_search_config: cocode_protocol::WebSearchConfig,
    /// Web fetch configuration.
    pub web_fetch_config: cocode_protocol::WebFetchConfig,
    /// Model-level cap on tool output size (characters).
    /// When set, applied after per-tool truncation but before persistence.
    pub max_tool_output_chars: Option<i32>,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        // Check environment variable for max concurrency override
        let max_concurrency = std::env::var("COCODE_MAX_TOOL_USE_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
            .filter(|&n| n > 0)
            .unwrap_or(DEFAULT_MAX_TOOL_CONCURRENCY);

        Self {
            max_concurrency,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            session_id: String::new(),
            turn_id: String::new(),
            turn_number: 0,
            permission_mode: PermissionMode::Default,
            default_timeout_secs: 120,
            is_plan_mode: false,
            is_ultraplan: false,
            plan_file_path: None,
            auto_memory_dir: None,
            session_dir: None,
            tool_config: cocode_protocol::ToolConfig::default(),
            features: cocode_protocol::Features::with_defaults(),
            web_search_config: cocode_protocol::WebSearchConfig::default(),
            web_fetch_config: cocode_protocol::WebFetchConfig::default(),
            max_tool_output_chars: None,
        }
    }
}

/// Pending tool call waiting for execution.
#[derive(Debug)]
struct PendingToolCall {
    tool_call: ToolCall,
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
pub struct StreamingToolExecutor {
    registry: Arc<ToolRegistry>,
    config: ExecutorConfig,
    event_tx: Option<mpsc::Sender<CoreEvent>>,
    cancel_token: CancellationToken,
    approval_store: Arc<Mutex<ApprovalStore>>,
    file_tracker: Arc<Mutex<FileTracker>>,
    /// Hook registry for pre/post tool hooks.
    hooks: Option<Arc<HookRegistry>>,
    /// Tracker for async hooks running in background.
    async_hook_tracker: Arc<AsyncHookTracker>,
    /// Active tool execution tasks.
    active_tasks: Arc<Mutex<HashMap<String, JoinHandle<ToolExecutionResult>>>>,
    /// Pending unsafe tools waiting for sequential execution.
    pending_unsafe: Arc<Mutex<Vec<PendingToolCall>>>,
    /// Completed results waiting to be collected.
    completed_results: Arc<Mutex<Vec<ToolExecutionResult>>>,
    /// Shell executor for command execution and background tasks.
    shell_executor: ShellExecutor,
    /// Sandbox state for platform-level command isolation.
    sandbox_state: Option<std::sync::Arc<cocode_sandbox::SandboxState>>,
    /// Optional callback for spawning subagents.
    spawn_agent_fn: Option<SpawnAgentFn>,
    /// Shared registry of cancellation tokens for background agents.
    agent_cancel_tokens: crate::context::AgentCancelTokens,
    /// Shared set of agent IDs killed via TaskStop.
    killed_agents: crate::context::KilledAgents,
    /// Base directory for background agent output files.
    agent_output_dir: Option<PathBuf>,
    /// Optional model call function for single-shot LLM calls.
    model_call_fn: Option<ModelCallFn>,
    /// Optional skill manager for the Skill tool.
    skill_manager: Option<Arc<cocode_skill::SkillManager>>,
    /// Parent selections for subagent isolation.
    ///
    /// When spawning subagents, these selections are passed to ensure
    /// subagents are unaffected by changes to the parent's model settings.
    parent_selections: Option<cocode_protocol::RoleSelections>,
    /// Optional permission requester for interactive approval flow.
    permission_requester: Option<Arc<dyn crate::context::PermissionRequester>>,
    /// Optional permission rule evaluator.
    permission_evaluator: Option<cocode_policy::PermissionRuleEvaluator>,
    /// Current batch ID for parallel tool execution grouping (UI only).
    ///
    /// A fresh UUID is generated in [`set_allowed_tool_names`] at the start of
    /// each streaming turn. All tools emitted during that turn — both safe
    /// (concurrent) and unsafe (sequential) — share the same batch ID. The TUI
    /// uses this to visually group tools that were dispatched together; it does
    /// not affect execution ordering.
    current_batch_id: Arc<std::sync::RwLock<Option<String>>>,
    /// Cancellation token for sibling abort propagation.
    ///
    /// When a Bash tool fails (`is_error == true`) during parallel execution,
    /// this token is cancelled to abort all other concurrent sibling tools in
    /// the same batch. Reset at the start of each streaming turn by replacing
    /// the inner token. Matches Claude Code's `siblingAbortController` pattern.
    sibling_abort_token: Arc<std::sync::RwLock<CancellationToken>>,
    /// Description of the tool that triggered sibling abort (for error messages).
    sibling_error_desc: Arc<std::sync::RwLock<Option<String>>>,

    /// Allowlist of tool names the model was actually given.
    ///
    /// Set after `select_tools_for_model()` via [`set_allowed_tool_names`].
    /// When `Some`, only these tools can be executed; all others get `NotFound`.
    /// When `None` (default), all registered tools are executable.
    allowed_tool_names: Arc<std::sync::RwLock<Option<HashSet<String>>>>,
    /// Shared invoked skills tracker across all tool contexts.
    ///
    /// When skills are invoked via the Skill tool, they are tracked here
    /// so the driver can inject them into system reminders.
    invoked_skills: Arc<Mutex<Vec<crate::context::InvokedSkill>>>,
    /// Skill-level tool restriction.
    ///
    /// When a skill with `allowed_tools` is invoked, this is set to restrict
    /// which tools can be used during the skill execution. Applied as an
    /// intersection with `allowed_tool_names`.
    skill_allowed_tools: Arc<std::sync::RwLock<Option<HashSet<String>>>>,
    /// Optional OTel manager for metrics and traces.
    otel_manager: Option<Arc<cocode_otel::OtelManager>>,
    /// Optional LSP server manager for language intelligence tools.
    lsp_manager: Option<Arc<cocode_lsp::LspServerManager>>,
    /// Allowed subagent types for the Task tool.
    ///
    /// Set from `Task(type1, type2)` syntax in the agent's tool allow-list.
    /// When `Some`, the Task tool only allows spawning the specified types.
    task_type_restrictions: Option<Vec<String>>,
    /// Optional file backup store for pre-modify snapshots (Tier 1 rewind).
    file_backup_store: Option<Arc<cocode_file_backup::FileBackupStore>>,
    /// Optional question responder for AskUserQuestion tool.
    question_responder: Option<Arc<crate::context::QuestionResponder>>,
    /// Path to the cocode home directory for durable cron persistence.
    cocode_home: Option<PathBuf>,
    /// Shared per-task byte offsets for incremental output reading.
    output_offsets: Arc<tokio::sync::Mutex<HashMap<String, u64>>>,
}

/// Result of pre-hook execution.
///
/// Carries the (possibly modified) input along with signals collected
/// from hook outcomes that the caller must act on.
struct PreHookOutcome {
    /// The (possibly modified) input.
    input: Value,
    /// If true, a hook returned PermissionOverride "allow" — skip permission checks.
    skip_permission: bool,
    /// Additional contexts collected from `ContinueWithContext` hooks.
    additional_contexts: Vec<String>,
}

/// Permission level for hook-based permission aggregation.
///
/// Most-restrictive-wins: deny(0) > ask(1) > allow(2) > undefined(3).
/// Lower ordinal = more restrictive.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum PermissionLevel {
    Deny = 0,
    Ask = 1,
    Allow = 2,
    Undefined = 3,
}

impl PermissionLevel {
    fn from_decision(s: &str) -> Self {
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
    /// Create a new executor.
    pub fn new(
        registry: Arc<ToolRegistry>,
        config: ExecutorConfig,
        event_tx: Option<mpsc::Sender<CoreEvent>>,
    ) -> Self {
        let shell_executor = ShellExecutor::new(config.cwd.clone());
        Self {
            registry,
            config,
            event_tx,
            cancel_token: CancellationToken::new(),
            approval_store: Arc::new(Mutex::new(ApprovalStore::new())),
            file_tracker: Arc::new(Mutex::new(FileTracker::new())),
            hooks: None,
            async_hook_tracker: Arc::new(AsyncHookTracker::new()),
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
            pending_unsafe: Arc::new(Mutex::new(Vec::new())),
            completed_results: Arc::new(Mutex::new(Vec::new())),
            shell_executor,
            sandbox_state: None,
            spawn_agent_fn: None,
            agent_cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
            killed_agents: Arc::new(Mutex::new(HashSet::new())),
            agent_output_dir: None,
            model_call_fn: None,
            skill_manager: None,
            parent_selections: None,
            permission_requester: None,
            permission_evaluator: None,
            current_batch_id: Arc::new(std::sync::RwLock::new(None)),
            sibling_abort_token: Arc::new(std::sync::RwLock::new(CancellationToken::new())),
            sibling_error_desc: Arc::new(std::sync::RwLock::new(None)),
            allowed_tool_names: Arc::new(std::sync::RwLock::new(None)),
            invoked_skills: Arc::new(Mutex::new(Vec::new())),
            skill_allowed_tools: Arc::new(std::sync::RwLock::new(None)),
            otel_manager: None,
            lsp_manager: None,
            task_type_restrictions: None,
            file_backup_store: None,
            question_responder: None,
            cocode_home: None,
            output_offsets: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Set the cancellation token.
    pub fn with_cancel_token(mut self, token: CancellationToken) -> Self {
        self.cancel_token = token;
        self
    }

    /// Set the hook registry for pre/post tool hooks.
    pub fn with_hooks(mut self, hooks: Arc<HookRegistry>) -> Self {
        // Wire the async tracker into the hook registry so background hooks
        // can deliver results via tracker.complete()
        hooks.set_async_hook_tracker(self.async_hook_tracker.clone());
        self.hooks = Some(hooks);
        self
    }

    /// Set the approval store.
    pub fn with_approval_store(mut self, store: Arc<Mutex<ApprovalStore>>) -> Self {
        self.approval_store = store;
        self
    }

    /// Set the file tracker.
    pub fn with_file_tracker(mut self, tracker: Arc<Mutex<FileTracker>>) -> Self {
        self.file_tracker = tracker;
        self
    }

    /// Set the shell executor for command execution and background tasks.
    pub fn with_shell_executor(mut self, executor: ShellExecutor) -> Self {
        self.shell_executor = executor;
        self
    }

    /// Set the sandbox state for platform-level command isolation.
    pub fn with_sandbox_state(
        mut self,
        state: std::sync::Arc<cocode_sandbox::SandboxState>,
    ) -> Self {
        self.sandbox_state = Some(state);
        self
    }

    /// Set the spawn agent callback for the Task tool.
    pub fn with_spawn_agent_fn(mut self, f: SpawnAgentFn) -> Self {
        self.spawn_agent_fn = Some(f);
        self
    }

    /// Set the shared agent cancel token registry.
    pub fn with_agent_cancel_tokens(mut self, tokens: crate::context::AgentCancelTokens) -> Self {
        self.agent_cancel_tokens = tokens;
        self
    }

    /// Set the shared killed agents registry.
    pub fn with_killed_agents(mut self, killed: crate::context::KilledAgents) -> Self {
        self.killed_agents = killed;
        self
    }

    /// Set the agent output directory for TaskOutput to find agent results.
    pub fn with_agent_output_dir(mut self, dir: PathBuf) -> Self {
        self.agent_output_dir = Some(dir);
        self
    }

    /// Set the model call function for single-shot LLM calls (SmartEdit).
    pub fn with_model_call_fn(mut self, f: ModelCallFn) -> Self {
        self.model_call_fn = Some(f);
        self
    }

    /// Set a custom async hook tracker.
    pub fn with_async_hook_tracker(mut self, tracker: Arc<AsyncHookTracker>) -> Self {
        // Wire to hook registry if already set, so background hooks can deliver results
        if let Some(ref hooks) = self.hooks {
            hooks.set_async_hook_tracker(tracker.clone());
        }
        self.async_hook_tracker = tracker;
        self
    }

    /// Set parent selections for subagent isolation.
    ///
    /// When spawning subagents via the Task tool, these selections will be
    /// cloned and passed to the subagent, ensuring it's unaffected by
    /// subsequent changes to the parent's model settings.
    pub fn with_parent_selections(mut self, selections: cocode_protocol::RoleSelections) -> Self {
        self.parent_selections = Some(selections);
        self
    }

    /// Set the permission requester for interactive approval flow.
    pub fn with_permission_requester(
        mut self,
        requester: Arc<dyn crate::context::PermissionRequester>,
    ) -> Self {
        self.permission_requester = Some(requester);
        self
    }

    /// Set the permission rule evaluator.
    pub fn with_permission_evaluator(
        mut self,
        evaluator: cocode_policy::PermissionRuleEvaluator,
    ) -> Self {
        self.permission_evaluator = Some(evaluator);
        self
    }

    /// Set the skill manager for the Skill tool.
    pub fn with_skill_manager(mut self, manager: Arc<cocode_skill::SkillManager>) -> Self {
        // Store in a way that can be passed to tool context
        // Note: The actual wiring happens in create_context
        self.skill_manager = Some(manager);
        self
    }

    /// Set the OTel manager for metrics and traces.
    pub fn with_otel_manager(mut self, otel: Option<Arc<cocode_otel::OtelManager>>) -> Self {
        self.otel_manager = otel;
        self
    }

    /// Set the LSP server manager for language intelligence tools.
    pub fn with_lsp_manager(mut self, manager: Arc<cocode_lsp::LspServerManager>) -> Self {
        self.lsp_manager = Some(manager);
        self
    }

    /// Set Task type restrictions for subagent spawning.
    ///
    /// When set, the Task tool will only allow spawning the specified agent types.
    pub fn with_task_type_restrictions(mut self, restrictions: Vec<String>) -> Self {
        self.task_type_restrictions = Some(restrictions);
        self
    }

    /// Set the file backup store for pre-modify snapshots.
    pub fn with_file_backup_store(
        mut self,
        store: Arc<cocode_file_backup::FileBackupStore>,
    ) -> Self {
        self.file_backup_store = Some(store);
        self
    }

    /// Set the question responder for AskUserQuestion tool.
    pub fn with_question_responder(
        mut self,
        responder: Arc<crate::context::QuestionResponder>,
    ) -> Self {
        self.question_responder = Some(responder);
        self
    }

    /// Set the cocode home directory for durable cron persistence.
    pub fn with_cocode_home(mut self, path: PathBuf) -> Self {
        self.cocode_home = Some(path);
        self
    }

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
            self.completed_results
                .lock()
                .await
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
                self.pending_unsafe
                    .lock()
                    .await
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
                // Check concurrency limit
                let active_count = self.active_tasks.lock().await.len();
                if active_count >= self.config.max_concurrency as usize {
                    // Queue instead of starting immediately
                    self.pending_unsafe
                        .lock()
                        .await
                        .push(PendingToolCall { tool_call });
                    return;
                }

                // Start immediately
                self.start_tool_execution(tool_call).await;
            }
            false => {
                // Queue for sequential execution
                self.pending_unsafe
                    .lock()
                    .await
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
                self.completed_results
                    .lock()
                    .await
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
                self.approval_store
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

        self.active_tasks
            .lock()
            .await
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
            let mut lock = self.pending_unsafe.lock().await;
            std::mem::take(&mut *lock)
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
                self.completed_results
                    .lock()
                    .await
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
                let active_count = self.active_tasks.lock().await.len();
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
        self.completed_results
            .lock()
            .await
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
            let mut lock = self.active_tasks.lock().await;
            lock.drain().collect()
        };

        for (call_id, handle) in tasks {
            match handle.await {
                Ok(result) => {
                    self.emit_completed(&result.call_id, &result.result).await;
                    self.completed_results.lock().await.push(result);
                }
                Err(e) if e.is_cancelled() => {
                    // Task was aborted (e.g. by sibling abort or user interrupt).
                    // Create a synthetic error result.
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
                    self.completed_results
                        .lock()
                        .await
                        .push(ToolExecutionResult {
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
                    self.completed_results
                        .lock()
                        .await
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
    }

    /// Wait for whichever active task finishes first and collect its result.
    ///
    /// Uses `select_all` to race all active handles so we don't block on an
    /// arbitrary slow task while faster ones are ready.
    async fn drain_one_active(&self) {
        let entries: Vec<(String, JoinHandle<ToolExecutionResult>)> = {
            let mut lock = self.active_tasks.lock().await;
            lock.drain().collect()
        };

        if entries.is_empty() {
            return;
        }

        let (ids, handles): (Vec<_>, Vec<_>) = entries.into_iter().unzip();

        let (join_result, winner_idx, remaining) = futures::future::select_all(handles).await;

        // Re-insert the handles that didn't finish yet.
        // `remaining` is in original order with the winner removed.
        {
            let mut lock = self.active_tasks.lock().await;
            let mut orig_idx = 0;
            for handle in remaining {
                if orig_idx == winner_idx {
                    orig_idx += 1;
                }
                lock.insert(ids[orig_idx].clone(), handle);
                orig_idx += 1;
            }
        }

        let call_id = &ids[winner_idx];
        match join_result {
            Ok(result) => {
                self.emit_completed(&result.call_id, &result.result).await;
                self.completed_results.lock().await.push(result);
            }
            Err(e) => {
                error!(call_id = %call_id, error = %e, "Task panicked");
                let result = Err(crate::error::tool_error::InternalSnafu {
                    message: format!("Tool execution task panicked (call_id: {call_id}): {e}"),
                }
                .build());
                self.emit_completed(call_id, &result).await;
                self.completed_results
                    .lock()
                    .await
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
        let mut results = self.completed_results.lock().await;
        std::mem::take(&mut *results)
    }

    /// Abort a running tool by call ID.
    pub async fn abort(&self, call_id: &str, reason: AbortReason) {
        // Cancel the token associated with this tool
        // Note: In a full implementation, each tool would have its own cancel token
        info!(call_id = %call_id, reason = ?reason, "Aborting tool");

        // Remove from active tasks
        if let Some(handle) = self.active_tasks.lock().await.remove(call_id) {
            handle.abort();
        }

        // Emit aborted event
        self.emit_tui(TuiEvent::ToolExecutionAborted { reason })
            .await;
    }

    /// Abort all running and pending tools.
    pub async fn abort_all(&self, reason: AbortReason) {
        // Cancel all active tasks
        let tasks: Vec<_> = {
            let mut lock = self.active_tasks.lock().await;
            lock.drain().collect()
        };

        for (_, handle) in tasks {
            handle.abort();
        }

        // Clear pending
        self.pending_unsafe.lock().await.clear();

        // Emit aborted event
        self.emit_tui(TuiEvent::ToolExecutionAborted { reason })
            .await;
    }

    /// Get the number of active tasks.
    pub async fn active_count(&self) -> usize {
        self.active_tasks.lock().await.len()
    }

    /// Get the number of pending unsafe tasks.
    pub async fn pending_count(&self) -> usize {
        self.pending_unsafe.lock().await.len()
    }

    /// Set a shared invoked skills tracker.
    ///
    /// The driver passes its own Arc so invoked skills persist across turns.
    pub fn set_invoked_skills(&mut self, skills: Arc<Mutex<Vec<crate::context::InvokedSkill>>>) {
        self.invoked_skills = skills;
    }

    /// Get the shared invoked skills tracker.
    ///
    /// Returns the Arc to the invoked skills list. After tool execution,
    /// the driver can read this to inject invoked skills into system reminders.
    pub fn invoked_skills(&self) -> &Arc<Mutex<Vec<crate::context::InvokedSkill>>> {
        &self.invoked_skills
    }

    /// Get the shared agent cancel token registry.
    ///
    /// The driver / subagent manager should register cancel tokens here
    /// when spawning agents, so TaskStop can cancel them by ID.
    pub fn agent_cancel_tokens(&self) -> &crate::context::AgentCancelTokens {
        &self.agent_cancel_tokens
    }

    /// Get the shared killed agents registry.
    pub fn killed_agents(&self) -> &crate::context::KilledAgents {
        &self.killed_agents
    }

    /// Create a tool context for execution.
    fn create_context(&self, call_id: &str) -> ToolContext {
        let mut builder = ToolContextBuilder::new(call_id, &self.config.session_id)
            .cwd(self.shell_executor.cwd())
            .turn_id(&self.config.turn_id)
            .turn_number(self.config.turn_number)
            .permission_mode(self.config.permission_mode)
            .cancel_token(self.cancel_token.clone())
            .approval_store(self.approval_store.clone())
            .file_tracker(self.file_tracker.clone())
            .plan_mode(self.config.is_plan_mode, self.config.plan_file_path.clone())
            .is_ultraplan(self.config.is_ultraplan)
            .auto_memory_dir(self.config.auto_memory_dir.clone())
            .features(self.config.features.clone())
            .web_search_config(self.config.web_search_config.clone())
            .web_fetch_config(self.config.web_fetch_config.clone())
            .shell_executor(self.shell_executor.clone())
            .invoked_skills(self.invoked_skills.clone());

        // Add spawn_agent_fn if available
        if let Some(ref spawn_fn) = self.spawn_agent_fn {
            builder = builder.spawn_agent_fn(spawn_fn.clone());
        }

        // Share agent cancel tokens and killed agents registries
        builder = builder
            .agent_cancel_tokens(self.agent_cancel_tokens.clone())
            .killed_agents(self.killed_agents.clone());

        // Add agent_output_dir if available
        if let Some(ref dir) = self.agent_output_dir {
            builder = builder.agent_output_dir(dir.clone());
        }

        // Add model_call_fn if available
        if let Some(ref call_fn) = self.model_call_fn {
            builder = builder.model_call_fn(call_fn.clone());
        }

        // Add skill_manager if available
        if let Some(ref sm) = self.skill_manager {
            builder = builder.skill_manager(sm.clone());
        }

        // Add lsp_manager if available
        if let Some(ref lm) = self.lsp_manager {
            builder = builder.lsp_manager(lm.clone());
        }

        // Add session_dir if available
        if let Some(ref dir) = self.config.session_dir {
            builder = builder.session_dir(dir.clone());
        }

        // Add parent_selections for subagent isolation
        if let Some(ref selections) = self.parent_selections {
            builder = builder.parent_selections(selections.clone());
        }

        // Add permission requester for interactive approval flow
        if let Some(ref requester) = self.permission_requester {
            builder = builder.permission_requester(requester.clone());
        }

        // Add permission rule evaluator
        if let Some(ref evaluator) = self.permission_evaluator {
            builder = builder.permission_evaluator(evaluator.clone());
        }

        // Add task type restrictions for the Task tool
        if let Some(ref restrictions) = self.task_type_restrictions {
            builder = builder.task_type_restrictions(restrictions.clone());
        }

        // Add file backup store for pre-modify snapshots
        if let Some(ref store) = self.file_backup_store {
            builder = builder.file_backup_store(store.clone());
        }

        // Add question responder for AskUserQuestion tool
        if let Some(ref responder) = self.question_responder {
            builder = builder.question_responder(responder.clone());
        }

        // Add cocode_home for durable cron persistence
        if let Some(ref home) = self.cocode_home {
            builder = builder.cocode_home(home.clone());
        }

        // Add sandbox state for platform-level command isolation
        builder = builder.maybe_sandbox_state(self.sandbox_state.clone());

        // Share output offsets across tool contexts for delta reads
        builder = builder.output_offsets(self.output_offsets.clone());

        builder.build()
    }

    async fn emit_protocol(&self, notif: ServerNotification) {
        if let Some(tx) = &self.event_tx
            && let Err(e) = tx.send(CoreEvent::Protocol(notif)).await
        {
            debug!("Failed to send protocol event: {e}");
        }
    }

    async fn emit_stream(&self, event: StreamEvent) {
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
    async fn emit_completed(&self, call_id: &str, result: &Result<ToolOutput>) {
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

    /// Execute pre-tool-use hooks and return the (possibly modified) input.
    ///
    /// Returns `Err` if the tool call should be rejected.
    async fn execute_pre_hooks(
        &self,
        tool_name: &str,
        tool_use_id: &str,
        input: Value,
    ) -> std::result::Result<PreHookOutcome, String> {
        let hooks = match &self.hooks {
            Some(h) => h,
            None => {
                return Ok(PreHookOutcome {
                    input,
                    skip_permission: false,
                    additional_contexts: Vec::new(),
                });
            }
        };

        let ctx = HookContext::new(
            HookEventType::PreToolUse,
            self.config.session_id.clone(),
            self.config.cwd.clone(),
        )
        .with_tool(tool_name, input.clone())
        .with_tool_use_id(tool_use_id);

        let outcomes = hooks.execute(&ctx).await;
        let mut current_input = input;
        let mut additional_contexts = Vec::new();
        let mut aggregated_permission = PermissionLevel::Undefined;
        let mut first_deny_reason: Option<String> = None;

        for outcome in outcomes {
            // Emit hook executed event
            self.emit_protocol(ServerNotification::HookExecuted(HookExecutedParams {
                hook_type: HookEventType::PreToolUse.to_string(),
                hook_name: outcome.hook_name.clone(),
            }))
            .await;

            match outcome.result {
                HookResult::Continue => {
                    // Continue with current input
                }
                HookResult::ContinueWithContext {
                    additional_context, ..
                } => {
                    if let Some(ctx_str) = additional_context {
                        additional_contexts.push(ctx_str);
                    }
                }
                HookResult::Reject { reason } => {
                    warn!(
                        tool = %tool_name,
                        hook = %outcome.hook_name,
                        reason = %reason,
                        "Tool call rejected by pre-hook"
                    );
                    return Err(reason);
                }
                HookResult::ModifyInput { new_input } => {
                    debug!(
                        tool = %tool_name,
                        hook = %outcome.hook_name,
                        "Tool input modified by pre-hook"
                    );
                    current_input = new_input;
                }
                HookResult::Async { task_id, hook_name } => {
                    // Register async hook for tracking - result will be delivered via system reminders
                    self.async_hook_tracker
                        .register(task_id.clone(), hook_name.clone());
                    debug!(
                        tool = %tool_name,
                        task_id = %task_id,
                        async_hook = %hook_name,
                        "Async hook registered and running in background"
                    );
                }
                HookResult::PermissionOverride { decision, reason } => {
                    debug!(
                        tool = %tool_name,
                        hook = %outcome.hook_name,
                        decision = %decision,
                        reason = ?reason,
                        "Hook returned permission override"
                    );
                    let level = PermissionLevel::from_decision(&decision);
                    // Most-restrictive-wins: lower ordinal = more restrictive
                    if level < aggregated_permission {
                        aggregated_permission = level;
                        if level == PermissionLevel::Deny && first_deny_reason.is_none() {
                            first_deny_reason = reason;
                        }
                    }
                }
                HookResult::SystemMessage { message } => {
                    debug!(
                        tool = %tool_name,
                        hook = %outcome.hook_name,
                        "Pre-hook system message: {message}"
                    );
                }
                HookResult::ModifyOutput { .. } | HookResult::PreventContinuation { .. } => {
                    // ModifyOutput and PreventContinuation are only relevant for PostToolUse, ignore in PreToolUse
                }
            }
        }

        // Apply aggregated permission decision
        match aggregated_permission {
            PermissionLevel::Deny => {
                return Err(first_deny_reason
                    .unwrap_or_else(|| "Tool denied by hook permission override".to_string()));
            }
            PermissionLevel::Ask => {
                // Explicitly ask — don't skip permission
            }
            PermissionLevel::Allow => {
                return Ok(PreHookOutcome {
                    input: current_input,
                    skip_permission: true,
                    additional_contexts,
                });
            }
            PermissionLevel::Undefined => {
                // No permission overrides — no change
            }
        }

        Ok(PreHookOutcome {
            input: current_input,
            skip_permission: false,
            additional_contexts,
        })
    }

    /// Execute post-tool-use hooks.
    async fn execute_post_hooks(
        &self,
        tool_name: &str,
        tool_use_id: &str,
        input: &Value,
        result: &Result<ToolOutput>,
    ) -> (PostHookAction, Vec<String>) {
        run_post_hooks(
            self.hooks.as_deref(),
            tool_name,
            tool_use_id,
            input,
            result,
            &self.config.session_id,
            &self.config.cwd,
        )
        .await
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

/// Shared post-hook execution logic.
///
/// Used by both `start_tool_execution` (spawned safe tools) and
/// `execute_single_tool` (inline unsafe tools) to ensure consistent behavior.
///
/// Returns a tuple of:
/// - [`PostHookAction`] indicating whether the result should be kept,
///   replaced with an error (Reject), substituted with hook-provided output
///   (ReplaceOutput), or the loop should halt (StopContinuation).
/// - `Vec<String>` of additional contexts collected from `ContinueWithContext` hooks.
async fn run_post_hooks(
    hooks: Option<&HookRegistry>,
    tool_name: &str,
    tool_use_id: &str,
    input: &Value,
    result: &Result<ToolOutput>,
    session_id: &str,
    cwd: &Path,
) -> (PostHookAction, Vec<String>) {
    let hooks = match hooks {
        Some(h) => h,
        None => return (PostHookAction::None, Vec::new()),
    };

    let is_error = result.is_err();
    let event_type = if is_error {
        HookEventType::PostToolUseFailure
    } else {
        HookEventType::PostToolUse
    };

    let mut ctx = HookContext::new(event_type, session_id.to_string(), cwd.to_path_buf())
        .with_tool(tool_name, input.clone())
        .with_tool_use_id(tool_use_id);

    // Enrich context with result-specific fields
    match result {
        Ok(output) => {
            // Convert ToolResultContent to serde_json::Value for tool_response
            if let Ok(response_value) = serde_json::to_value(&output.content) {
                ctx = ctx.with_tool_response(response_value);
            }
        }
        Err(e) => {
            ctx = ctx.with_error(e.to_string());
        }
    }

    let outcomes = hooks.execute(&ctx).await;
    let mut action = PostHookAction::None;
    let mut additional_contexts = Vec::new();
    for outcome in outcomes {
        match outcome.result {
            HookResult::Reject { reason } => {
                warn!(
                    tool = %tool_name,
                    hook = %outcome.hook_name,
                    reason = %reason,
                    "PostToolUse hook rejected — tool output will be replaced with error"
                );
                action = PostHookAction::Reject(reason);
            }
            HookResult::ModifyOutput { new_output } => {
                debug!(
                    tool = %tool_name,
                    hook = %outcome.hook_name,
                    "PostToolUse hook provided replacement output"
                );
                let text = new_output
                    .as_str()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| new_output.to_string());
                action = PostHookAction::ReplaceOutput(ToolOutput::text(text));
            }
            HookResult::ContinueWithContext {
                additional_context: Some(ctx_str),
                ..
            } => {
                additional_contexts.push(ctx_str);
            }
            HookResult::PreventContinuation { reason } => {
                let reason_text = reason.unwrap_or_else(|| outcome.hook_name.clone());
                info!(
                    tool = %tool_name,
                    hook = %outcome.hook_name,
                    reason = %reason_text,
                    "PostToolUse hook requested loop stop — tool output preserved"
                );
                action = PostHookAction::StopContinuation(reason_text);
            }
            _ => {}
        }
    }
    (action, additional_contexts)
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
    let cancel_token = ctx.cancel_token.clone();

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
    if name.starts_with("mcp__") {
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
/// E.g. `"git push origin main"` → `Some("git *")`.
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
fn default_approval_request(name: &str, input: &Value) -> cocode_protocol::ApprovalRequest {
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
/// 1. Check DENY rules (all sources) → if match → Deny
/// 2. Check ASK rules (all sources) → if match → NeedsApproval
/// 3. Tool-specific check_permission() → returns allow/deny/ask/passthrough
/// 4. Check ALLOW rules (all sources) → if match → Allow
/// 5. Default behavior: reads → Allow, writes → NeedsApproval
async fn check_permission_pipeline(
    tool: &dyn crate::tool::Tool,
    name: &str,
    input: &Value,
    ctx: &ToolContext,
) -> cocode_protocol::PermissionResult {
    let file_path = extract_file_path(input);
    let command_input = extract_command_input(name, input);

    if let Some(ref evaluator) = ctx.permission_evaluator {
        // Stages 1+2: Check DENY then ASK rules
        if let Some(decision) =
            evaluator.evaluate_deny_ask(name, file_path.as_deref(), command_input.as_deref())
        {
            if decision.result.is_denied() {
                return decision.result;
            }
            // ASK rule matched — the tool must ask for approval
            return cocode_protocol::PermissionResult::NeedsApproval {
                request: cocode_protocol::ApprovalRequest {
                    request_id: format!("rule-ask-{name}"),
                    tool_name: name.to_string(),
                    description: decision.reason,
                    risks: vec![],
                    allow_remember: true,
                    proposed_prefix_pattern: extract_prefix_pattern(name, input),
                    input: Some(input.clone()),
                },
            };
        }
    }

    // Stage 3: Tool-specific check
    let tool_result = tool.check_permission(input, ctx).await;
    if !tool_result.is_passthrough() {
        return tool_result;
    }

    if let Some(ref evaluator) = ctx.permission_evaluator {
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
            request: default_approval_request(name, input),
        }
    }
}

/// Apply permission mode on top of pipeline result.
///
/// Converts results based on the current mode:
/// - Bypass: everything except Denied → Allowed (deny is absolute)
/// - DontAsk: NeedsApproval → Denied
/// - AcceptEdits: edit/write NeedsApproval → Allowed
/// - Plan: non-read-only → Denied
fn apply_permission_mode(
    result: cocode_protocol::PermissionResult,
    mode: PermissionMode,
    tool_name: &str,
    registry: &ToolRegistry,
) -> cocode_protocol::PermissionResult {
    match mode {
        // Bypass allows everything EXCEPT explicit denials — deny is absolute.
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
            // In plan mode, deny non-read-only tools — but respect tool-level Allowed
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
        && !ctx.features.enabled(feature)
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
    let permission = apply_permission_mode(pipeline_result, ctx.permission_mode, name, registry);

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
            } else if let Some(hooks) = &ctx.hook_registry
                && let Some(override_decision) = {
                    // Fire PermissionRequest hooks before interactive approval
                    let perm_ctx = HookContext::new(
                        HookEventType::PermissionRequest,
                        ctx.session_id.clone(),
                        ctx.cwd.clone(),
                    )
                    .with_tool(name, input.clone())
                    .with_tool_use_id(call_id)
                    .with_permission_mode(match ctx.permission_mode {
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
                        // "ask" or unknown — fall through to interactive approval
                    }
                }
            } else if let Some(requester) = &ctx.permission_requester {
                // Use the permission requester for interactive approval
                let worker_id = ctx.call_id.clone();
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
            // Should not happen after pipeline — treat as allowed
        }
    }

    // Pre-execute file backup (Tier 1 rewind)
    if !tool.is_read_only()
        && let Some(ref backup_store) = ctx.file_backup_store
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
    // - A small <persisted-output> block (if persisted) → truncation is a no-op
    // - The original output ≤ per_tool_limit → truncation only fires if model_limit is smaller
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
