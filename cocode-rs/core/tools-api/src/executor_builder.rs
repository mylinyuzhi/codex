//! Builder pattern and configuration for [`StreamingToolExecutor`].
//!
//! Contains [`ExecutorConfig`] with its `Default` impl, and all `with_*`
//! builder methods on `StreamingToolExecutor`.

use crate::context::ModelCallFn;
use crate::context::SpawnAgentFn;
use crate::registry::ToolRegistry;
use cocode_hooks::AsyncHookTracker;
use cocode_hooks::HookRegistry;
use cocode_policy::ApprovalStore;
use cocode_protocol::PermissionMode;
use cocode_shell::ShellExecutor;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::context::FileTracker;
use crate::executor::DEFAULT_MAX_TOOL_CONCURRENCY;
use crate::executor::StreamingToolExecutor;
use crate::executor::ToolExecutionState;
use cocode_protocol::CoreEvent;

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
    /// Whether cowork mode is active (disables memory write bypass).
    pub is_cowork_mode: bool,
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
    /// Allowed subagent types for the Task tool.
    ///
    /// Set from `Task(type1, type2)` syntax in the agent's tool allow-list.
    /// When `Some`, the Task tool only allows spawning the specified types.
    pub task_type_restrictions: Option<Vec<String>>,
    /// Team name when running as a teammate.
    ///
    /// When set, tools like ExitPlanMode route approval through the team
    /// mailbox instead of showing a user-facing dialog.
    pub team_name: Option<String>,
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
            is_cowork_mode: false,
            session_dir: None,
            tool_config: cocode_protocol::ToolConfig::default(),
            features: cocode_protocol::Features::with_defaults(),
            web_search_config: cocode_protocol::WebSearchConfig::default(),
            web_fetch_config: cocode_protocol::WebFetchConfig::default(),
            max_tool_output_chars: None,
            task_type_restrictions: None,
            team_name: None,
        }
    }
}

impl StreamingToolExecutor {
    /// Create a new executor.
    pub fn new(
        registry: Arc<ToolRegistry>,
        config: ExecutorConfig,
        event_tx: Option<mpsc::Sender<CoreEvent>>,
    ) -> Self {
        let shell_executor = ShellExecutor::new(config.cwd.clone());
        let paths = crate::context::SessionPaths {
            session_dir: config.session_dir.clone(),
            cocode_home: None,
            auto_memory_dir: config.auto_memory_dir.clone(),
            plan_file_path: config.plan_file_path.clone(),
            is_cowork_mode: config.is_cowork_mode,
        };
        Self {
            registry,
            config,
            event_tx,
            cancel_token: CancellationToken::new(),
            hooks: None,
            async_hook_tracker: Arc::new(AsyncHookTracker::new()),
            executor_state: Arc::new(Mutex::new(ToolExecutionState::new())),
            services: crate::context::ToolServices {
                shell_executor,
                sandbox_state: None,
                lsp_manager: None,
                skill_manager: None,
                skill_usage_tracker: None,
                hook_registry: None,
                permission_requester: None,
                permission_evaluator: None,
                file_backup_store: None,
                question_responder: None,
            },
            state: crate::context::ToolSharedState {
                approval_store: Arc::new(Mutex::new(ApprovalStore::new())),
                file_tracker: Arc::new(Mutex::new(FileTracker::new())),
                invoked_skills: Arc::new(Mutex::new(Vec::new())),
                output_offsets: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            },
            agent: crate::context::AgentContext {
                spawn_agent_fn: None,
                agent_cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
                killed_agents: Arc::new(Mutex::new(HashSet::new())),
                agent_output_dir: None,
                model_call_fn: None,
                parent_selections: None,
                team_name: None,
            },
            current_batch_id: Arc::new(std::sync::RwLock::new(None)),
            sibling_abort_token: Arc::new(std::sync::RwLock::new(CancellationToken::new())),
            sibling_error_desc: Arc::new(std::sync::RwLock::new(None)),
            allowed_tool_names: Arc::new(std::sync::RwLock::new(None)),
            skill_allowed_tools: Arc::new(std::sync::RwLock::new(None)),
            otel_manager: None,
            paths,
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
        self.state.approval_store = store;
        self
    }

    /// Set the file tracker.
    pub fn with_file_tracker(mut self, tracker: Arc<Mutex<FileTracker>>) -> Self {
        self.state.file_tracker = tracker;
        self
    }

    /// Set the shell executor for command execution and background tasks.
    pub fn with_shell_executor(mut self, executor: ShellExecutor) -> Self {
        self.services.shell_executor = executor;
        self
    }

    /// Set the sandbox state for platform-level command isolation.
    pub fn with_sandbox_state(
        mut self,
        state: std::sync::Arc<cocode_sandbox::SandboxState>,
    ) -> Self {
        self.services.sandbox_state = Some(state);
        self
    }

    /// Set the spawn agent callback for the Task tool.
    pub fn with_spawn_agent_fn(mut self, f: SpawnAgentFn) -> Self {
        self.agent.spawn_agent_fn = Some(f);
        self
    }

    /// Set the shared agent cancel token registry.
    pub fn with_agent_cancel_tokens(mut self, tokens: crate::context::AgentCancelTokens) -> Self {
        self.agent.agent_cancel_tokens = tokens;
        self
    }

    /// Set the shared killed agents registry.
    pub fn with_killed_agents(mut self, killed: crate::context::KilledAgents) -> Self {
        self.agent.killed_agents = killed;
        self
    }

    /// Set the agent output directory for TaskOutput to find agent results.
    pub fn with_agent_output_dir(mut self, dir: PathBuf) -> Self {
        self.agent.agent_output_dir = Some(dir);
        self
    }

    /// Set the model call function for single-shot LLM calls (SmartEdit).
    pub fn with_model_call_fn(mut self, f: ModelCallFn) -> Self {
        self.agent.model_call_fn = Some(f);
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
        self.agent.parent_selections = Some(selections);
        self
    }

    /// Set the permission requester for interactive approval flow.
    pub fn with_permission_requester(
        mut self,
        requester: Arc<dyn crate::context::PermissionRequester>,
    ) -> Self {
        self.services.permission_requester = Some(requester);
        self
    }

    /// Set the permission rule evaluator.
    pub fn with_permission_evaluator(
        mut self,
        evaluator: cocode_policy::PermissionRuleEvaluator,
    ) -> Self {
        self.services.permission_evaluator = Some(evaluator);
        self
    }

    /// Set the skill manager for the Skill tool.
    pub fn with_skill_manager(mut self, manager: Arc<cocode_skill::SkillManager>) -> Self {
        self.services.skill_manager = Some(manager);
        self
    }

    /// Set the OTel manager for metrics and traces.
    pub fn with_otel_manager(mut self, otel: Option<Arc<cocode_otel::OtelManager>>) -> Self {
        self.otel_manager = otel;
        self
    }

    /// Set the LSP server manager for language intelligence tools.
    pub fn with_lsp_manager(mut self, manager: Arc<cocode_lsp::LspServerManager>) -> Self {
        self.services.lsp_manager = Some(manager);
        self
    }

    /// Set Task type restrictions for subagent spawning.
    ///
    /// When set, the Task tool will only allow spawning the specified agent types.
    pub fn with_task_type_restrictions(mut self, restrictions: Vec<String>) -> Self {
        self.config.task_type_restrictions = Some(restrictions);
        self
    }

    /// Set the file backup store for pre-modify snapshots.
    pub fn with_file_backup_store(
        mut self,
        store: Arc<cocode_file_backup::FileBackupStore>,
    ) -> Self {
        self.services.file_backup_store = Some(store);
        self
    }

    /// Set the question responder for AskUserQuestion tool.
    pub fn with_question_responder(
        mut self,
        responder: Arc<crate::context::QuestionResponder>,
    ) -> Self {
        self.services.question_responder = Some(responder);
        self
    }

    /// Set the cocode home directory for durable cron persistence.
    pub fn with_cocode_home(mut self, path: PathBuf) -> Self {
        self.paths.cocode_home = Some(path);
        self
    }
}

#[cfg(test)]
#[path = "executor_builder.test.rs"]
mod tests;
