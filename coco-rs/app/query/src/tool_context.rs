//! `ToolContextFactory` — single owner of `ToolUseContext` construction.
//!
//! TS: `services/tools/toolExecution.ts` builds the per-call context from the
//! `query()` loop's shared state. Rust keeps the same discipline: one factory
//! owns the field mapping, and the engine just asks for a fresh context per
//! turn.
//!
//! The factory exists so the refactor plan's I6 invariant (ToolUseContext is
//! accurate, not hardcoded defaults) is enforceable by tests — callers can
//! construct a factory from a subset of engine state and verify the fields
//! propagate without spinning up a full `QueryEngine`.
//!
//! # Fields fixed by this factory (I6)
//!
//! - `thinking_level`, `is_non_interactive`, `max_budget_usd`,
//!   `custom_system_prompt`, `append_system_prompt` — previously hardcoded
//!   defaults in `engine.rs`. Now mirrored from `QueryEngineConfig`.
//! - `main_loop_model` snapshots the currently-active model. The engine
//!   passes `ToolContextOverrides.current_model_name` at build time from
//!   `ModelRuntime::current_model_name()` so post-fallback contexts reflect
//!   the active slot. Callers that omit the override (tests, legacy single-
//!   client paths) fall back to `config.model_name`.
//! - `hook_handle` — plumbed through even when `None` so Phase 3's
//!   `QueryHookHandle` slots in without a second call-site edit.
//! - Permission-mode-related fields (`mode`, `pre_plan_mode`,
//!   `stripped_dangerous_rules`) are seeded from live `ToolAppState`
//!   when present so mid-session mutations (e.g. `EnterPlanMode`) are
//!   visible on the next batch.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use coco_config::EnvKey;
use coco_config::env;
use coco_context::FileHistoryState;
use coco_tool_runtime::AgentHandleRef;
use coco_tool_runtime::HookHandleRef;
use coco_tool_runtime::MailboxHandleRef;
use coco_tool_runtime::SkillHandleRef;
use coco_tool_runtime::TaskListHandleRef;
use coco_tool_runtime::TodoListHandleRef;
use coco_tool_runtime::ToolPermissionBridgeRef;
use coco_tool_runtime::ToolRegistry;
use coco_tool_runtime::ToolUseContext;
use coco_types::AgentId;
use coco_types::AppStateReadHandle;
use coco_types::ToolAppState;
use coco_types::ToolPermissionContext;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::config::QueryEngineConfig;

/// Immutable inputs needed to build a `ToolUseContext`.
///
/// This mirrors the QueryEngine state that is relevant to tool execution,
/// minus per-call overrides (which go through [`ToolContextOverrides`]).
///
/// All `Arc`/`Option<Arc<_>>` fields are cheap to clone; the factory keeps them
/// by value so a caller can construct one factory and reuse it across turns.
pub(crate) struct ToolContextFactory {
    pub(crate) config: QueryEngineConfig,
    pub(crate) tools: Arc<ToolRegistry>,
    pub(crate) cancel: CancellationToken,
    pub(crate) mailbox: Option<MailboxHandleRef>,
    pub(crate) task_list: Option<TaskListHandleRef>,
    pub(crate) todo_list: Option<TodoListHandleRef>,
    pub(crate) permission_bridge: Option<ToolPermissionBridgeRef>,
    pub(crate) app_state: Option<Arc<RwLock<ToolAppState>>>,
    pub(crate) file_read_state: Option<Arc<RwLock<coco_context::FileReadState>>>,
    pub(crate) file_history: Option<Arc<RwLock<FileHistoryState>>>,
    pub(crate) config_home: Option<PathBuf>,
    /// Optional hook-callback bridge. `None` in tests and phases where the
    /// adapter is not yet installed; filled in by Phase 3's `QueryHookHandle`.
    pub(crate) hook_handle: Option<HookHandleRef>,
    /// Optional agent-runtime handle. `None` resolves to
    /// `NoOpAgentHandle`, which returns "not available" errors for
    /// every `AgentTool` subcommand — suitable for tests and
    /// sessions that intentionally disable subagent spawning. The
    /// CLI / SDK / TUI runners install a real handle (the
    /// swarm-backed `SwarmAgentHandle`) at session bootstrap so
    /// `ctx.agent.spawn_agent(...)` reaches the real runtime.
    pub(crate) agent_handle: Option<AgentHandleRef>,
    /// Optional skill-runtime handle. `None` resolves to
    /// `NoOpSkillHandle`, which returns `Unavailable` from every
    /// `invoke_skill` call — the model sees a clean error instead
    /// of silent skipping. Swap in a real handle once `SkillRuntime`
    /// implementations land (Phase 7-β).
    pub(crate) skill_handle: Option<SkillHandleRef>,
    /// Session-scoped JSON Schema validator for tool inputs.
    /// Plan Phase 4a / I3: caches compiled `jsonschema::Validator`
    /// per `ToolId`; the preparer runs it on model input AND on
    /// PreToolUse hook-rewritten input to guarantee that
    /// malformed rewrites reject BEFORE permission / execution.
    pub(crate) tool_schema_validator: Option<coco_tool_runtime::ToolSchemaValidator>,
}

/// Per-call overrides applied on top of [`ToolContextFactory`] inputs.
#[derive(Default)]
pub(crate) struct ToolContextOverrides {
    /// UUID of the user message that drove this turn; threaded through so
    /// `file_history` snapshots key on the correct message.
    pub(crate) user_message_id: Option<String>,
    /// Per-turn progress channel. Tools (e.g. `Bash`) clone this via
    /// `ctx.progress_tx` and push `ToolProgress` updates; the engine
    /// spawns a drain task that forwards those updates to `event_tx`
    /// as `TuiOnlyEvent::ToolProgress` events.
    ///
    /// TS parity: `onProgress` callback passed per-turn into
    /// `StreamingToolExecutor`. Absent (`None`) ⇒ tools run without
    /// progress reporting (equivalent to the pre-Phase-9 baseline).
    pub(crate) progress_tx: Option<coco_tool_runtime::ProgressSender>,
    /// Currently-active model name. Engine passes
    /// `ModelRuntime::current_model_name()` so `main_loop_model`
    /// reflects post-fallback state; absent falls back to
    /// `config.model_name` (tests, pre-fallback paths).
    pub(crate) current_model_name: Option<String>,
}

impl ToolContextFactory {
    /// Build a fresh `ToolUseContext` for the next tool batch.
    ///
    /// Reads permission-mode-related fields from `app_state` when available —
    /// the prior batch's `ExitPlanModeTool` / `EnterPlanModeTool` patches
    /// become visible here without a config reload.
    pub(crate) async fn build(&self, overrides: ToolContextOverrides) -> ToolUseContext {
        let (live_mode, live_pre_plan, live_stripped) = match self.app_state.as_ref() {
            Some(state) => {
                let guard = state.read().await;
                (
                    guard.permission_mode.unwrap_or(self.config.permission_mode),
                    guard.pre_plan_mode,
                    guard.stripped_dangerous_rules.clone(),
                )
            }
            None => (self.config.permission_mode, None, None),
        };

        let plans_dir = self.config_home.as_ref().map(|ch| {
            coco_context::resolve_plans_directory(
                ch,
                self.config.project_dir.as_deref(),
                self.config.plans_directory.as_deref(),
            )
        });
        let session_plan_file = self.config_home.as_ref().map(|ch| {
            let dir = coco_context::resolve_plans_directory(
                ch,
                self.config.project_dir.as_deref(),
                self.config.plans_directory.as_deref(),
            );
            coco_context::get_plan_file_path(
                &self.config.session_id,
                &dir,
                self.config.agent_id.as_deref(),
            )
        });

        let main_loop_model = overrides
            .current_model_name
            .unwrap_or_else(|| self.config.model_name.clone());
        ToolUseContext {
            tools: self.tools.clone(),
            main_loop_model,
            // Honor the config-driven values that the previous inline
            // constructor hardcoded. TS parity: these always flow from
            // `queryConfig.*` through `toolUseContext.options.*`.
            thinking_level: self.config.thinking_level.clone(),
            is_non_interactive: self.config.is_non_interactive,
            max_budget_usd: self.config.max_budget_usd,
            custom_system_prompt: self.config.system_prompt.clone(),
            append_system_prompt: self.config.append_system_prompt.clone(),
            debug: false,
            verbose: false,
            tool_config: self.config.tool_config.clone(),
            sandbox_config: self.config.sandbox_config.clone(),
            memory_config: self.config.memory_config.clone(),
            shell_config: self.config.shell_config.clone(),
            web_fetch_config: self.config.web_fetch_config.clone(),
            web_search_config: self.config.web_search_config.clone(),
            is_teammate: self.config.is_teammate,
            plan_mode_required: self.config.plan_mode_required,
            // Pre-resolve swarm identity once, so tools read from ctx
            // instead of process env. Falls back to env vars set by the
            // teammate spawner for cross-process scenarios. Env namespace
            // is `COCO_*` — see swarm_constants.
            agent_name: env::env_opt(EnvKey::CocoAgentName)
                .or_else(|| self.config.agent_id.clone()),
            team_name: env::env_opt(EnvKey::CocoTeamName),
            plan_verify_execution: self.config.plan_mode_settings.verify_execution,
            cancel: self.cancel.clone(),
            messages: Arc::new(RwLock::new(Vec::new())),
            permission_context: ToolPermissionContext {
                mode: live_mode,
                additional_dirs: HashMap::new(),
                // Permission rules from settings.json (user /
                // project / policy). TS parity: loaded via
                // `loadPermissionRules` at session bootstrap and
                // passed verbatim to every tool invocation's
                // `toolPermissionContext`. Plan Tier 3 polish.
                allow_rules: self.config.allow_rules.clone(),
                deny_rules: self.config.deny_rules.clone(),
                ask_rules: self.config.ask_rules.clone(),
                bypass_available: self.config.bypass_permissions_available,
                pre_plan_mode: live_pre_plan,
                stripped_dangerous_rules: live_stripped,
                session_plan_file,
            },
            tool_use_id: None,
            user_message_id: overrides.user_message_id,
            agent_id: self.config.agent_id.as_ref().map(AgentId::new),
            agent_type: None,
            file_reading_limits: Default::default(),
            glob_limits: Default::default(),
            nested_memory_attachment_triggers: Arc::new(RwLock::new(Default::default())),
            loaded_nested_memory_paths: Default::default(),
            dynamic_skill_dir_triggers: Arc::new(RwLock::new(Default::default())),
            discovered_skill_names: Default::default(),
            tool_decisions: Default::default(),
            user_modified: false,
            require_can_use_tool: false,
            preserve_tool_use_results: false,
            rendered_system_prompt: None,
            critical_system_reminder: None,
            in_progress_tool_use_ids: Arc::new(RwLock::new(Default::default())),
            side_query: Arc::new(coco_tool_runtime::NoOpSideQuery),
            mcp: Arc::new(coco_tool_runtime::NoOpMcpHandle),
            schedules: Arc::new(coco_tool_runtime::NoOpScheduleStore),
            agent: self
                .agent_handle
                .clone()
                .unwrap_or_else(|| Arc::new(coco_tool_runtime::NoOpAgentHandle)),
            skill: self
                .skill_handle
                .clone()
                .unwrap_or_else(|| Arc::new(coco_tool_runtime::NoOpSkillHandle)),
            tool_schema_validator: self.tool_schema_validator.clone(),
            mailbox: self
                .mailbox
                .clone()
                .unwrap_or_else(|| Arc::new(coco_tool_runtime::NoOpMailboxHandle)),
            // Phase 6 Workstream C hook: worktree-isolated subagents
            // receive `cwd_override` via their child engine config so
            // relative-path-resolving tools (Glob/Grep/Bash) operate
            // inside the worktree. Absolute-path tools ignore this
            // field by design (schema enforces absolute paths),
            // matching TS behavior where `AsyncLocalStorage`-based
            // cwd override only affects resolving calls.
            cwd_override: self.config.cwd_override.clone(),
            permission_bridge: self.permission_bridge.clone(),
            progress_tx: overrides.progress_tx,
            task_handle: None,
            task_list: self
                .task_list
                .clone()
                .unwrap_or_else(|| Arc::new(coco_tool_runtime::NoOpTaskListHandle)),
            todo_list: self
                .todo_list
                .clone()
                .unwrap_or_else(|| Arc::new(coco_tool_runtime::InMemoryTodoListHandle::new())),
            hook_handle: self.hook_handle.clone(),
            file_read_state: self.file_read_state.clone(),
            file_history: self.file_history.clone(),
            config_home: self.config_home.clone(),
            session_id_for_history: Some(self.config.session_id.clone()),
            plans_dir,
            app_state: self
                .app_state
                .as_ref()
                .map(|arc| AppStateReadHandle::new(arc.clone())),
            local_denial_tracking: None,
            query_chain_id: None,
            query_depth: 0,
        }
    }
}

#[cfg(test)]
#[path = "tool_context.test.rs"]
mod tests;
