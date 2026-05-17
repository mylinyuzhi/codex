//! Configuration + result types for `QueryEngine`.
//!
//! Extracted from `engine.rs` so the orchestration module stays focused on the
//! session loop. Pure data types — no behavior lives here.

use coco_config::MemoryConfig;
use coco_config::PlanModeSettings;
use coco_config::SandboxSettings;
use coco_config::ShellConfig;
use coco_config::ToolConfig;
use coco_config::WebFetchConfig;
use coco_config::WebSearchConfig;
use coco_messages::CostTracker;
use coco_messages::Message;
use coco_types::Features;
use coco_types::PermissionMode;
use coco_types::PermissionRuleSource;
use coco_types::PermissionRulesBySource;
use coco_types::PromptCacheConfig;
use coco_types::ThinkingLevel;
use coco_types::TokenUsage;
use coco_types::ToolFilter;
use coco_types::ToolOverrides;
use std::sync::Arc;

/// Escalated max_output_tokens on first `length` stop. TS: `utils/context.ts:25`
/// `ESCALATED_MAX_TOKENS = 64_000`.
pub(crate) const ESCALATED_MAX_TOKENS: i64 = 64_000;

/// Hard cap on how many recovery cycles (post-escalation) we attempt before
/// giving up. TS: `query.ts:164` `MAX_OUTPUT_TOKENS_RECOVERY_LIMIT = 3`.
pub(crate) const MAX_OUTPUT_TOKENS_RECOVERY_LIMIT: i32 = 3;

/// Why the loop is continuing instead of exiting.
///
/// TS: Continue type union in query.ts — enables tests to verify recovery
/// paths fired without inspecting message contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinueReason {
    /// Normal tool-call loop: model returned tool calls, process and continue.
    NextTurn,
    /// Reactive compaction after prompt-too-long error.
    ReactiveCompactRetry,
    /// Max output tokens escalation (try 64k).
    MaxOutputTokensEscalate,
    /// Max output tokens recovery attempt.
    MaxOutputTokensRecovery { attempt: i32 },
    /// Stop hook requested blocking continuation.
    StopHookBlocking,
    /// Token budget allows one more continuation.
    TokenBudgetContinuation,
    /// Context collapse drain retry.
    CollapseDrainRetry { committed: i32 },
}

/// Configuration for the query engine.
#[derive(Debug, Clone)]
pub struct QueryEngineConfig {
    /// Maximum turns before stopping.
    pub max_turns: i32,
    /// Maximum output tokens per request.
    pub max_tokens: Option<i64>,
    /// Per-call prompt-cache directive. Main sessions set this when the
    /// active provider/model supports prompt caching; forked sessions
    /// inherit it from the parent cache slot and may set
    /// `skip_cache_write`.
    pub prompt_cache: Option<PromptCacheConfig>,
    /// System prompt to prepend.
    pub system_prompt: Option<String>,
    /// Append to system prompt (after CLAUDE.md).
    pub append_system_prompt: Option<String>,
    /// Model name for tool context.
    pub model_id: String,
    /// Permission mode for tool execution.
    pub permission_mode: PermissionMode,
    /// Whether this session may transition into `BypassPermissions`.
    ///
    /// Static capability set once at session bootstrap from the CLI
    /// (`--dangerously-skip-permissions` OR `--allow-dangerously-skip-permissions`)
    /// and policy killswitch. Threaded into
    /// `ToolPermissionContext.bypass_available` on every tool-context
    /// rebuild so the Plan-mode auto-allow + Shift+Tab cycle gate stay
    /// aligned.
    ///
    /// TS parity: `ToolPermissionContext.isBypassPermissionsModeAvailable`
    /// from `permissionSetup.ts:939-943`.
    pub bypass_permissions_available: bool,
    /// Context window size in tokens (for compaction trigger).
    pub context_window: i64,
    /// Max output tokens for the model (used in effective window calculation).
    pub max_output_tokens: i64,
    /// Maximum budget in USD (None = unlimited).
    pub max_budget_usd: Option<f64>,
    /// Enable streaming tool execution (tools execute during API streaming).
    pub streaming_tool_execution: bool,
    /// Whether this is a non-interactive (SDK/script) session.
    pub is_non_interactive: bool,
    /// Debug-logging surface for tools. Mirrors TS `toolUseContext.options.debug`
    /// (CLI `--debug`) — visible on `ToolUseContext.debug`. Defaults to `false`.
    pub debug: bool,
    /// Verbose-logging surface for tools. Mirrors TS
    /// `toolUseContext.options.verbose` (CLI `--verbose`) — visible on
    /// `ToolUseContext.verbose`. Defaults to `false`.
    pub verbose: bool,
    /// Thinking level applied to the main-loop model for this session.
    /// Surfaced on `ToolUseContext.thinking_level` so tools (and tool-
    /// spawned subqueries) see the same reasoning budget the engine is
    /// currently driving the LLM with. TS: `queryConfig.thinkingLevel`
    /// threaded through `toolUseContext.options.thinkingLevel`.
    pub thinking_level: Option<ThinkingLevel>,
    /// Session identifier for hook orchestration context.
    pub session_id: String,
    /// Project root directory for hook orchestration context.
    pub project_dir: Option<std::path::PathBuf>,
    /// Session-scoped permission rules loaded from settings.json
    /// (user / project / policy layers). Populated by the CLI
    /// layer at bootstrap; `ToolContextFactory` threads them into
    /// every `ToolUseContext.permission_context.{allow_rules,
    /// deny_rules, ask_rules}` so the evaluator sees the same
    /// rule set TS's `loadPermissionRules` would produce.
    ///
    /// Default-empty maps preserve the pre-wiring behavior where
    /// mode-based auto-allow (Plan / Accept / Bypass) was the only
    /// effective permission driver.
    pub allow_rules: PermissionRulesBySource,
    pub deny_rules: PermissionRulesBySource,
    pub ask_rules: PermissionRulesBySource,
    /// Root directories used to resolve leading-`/` path permission
    /// patterns per rule source. TS:
    /// `settings.ts::getSettingsRootPathForSource` + filesystem
    /// `rootPathForSource`; user settings resolve at config home,
    /// flag settings at the flag file dirname, and project/local/policy
    /// at original cwd.
    pub permission_rule_source_roots:
        std::collections::HashMap<PermissionRuleSource, std::path::PathBuf>,
    /// Per-session working-directory allowlist, augmenting the cwd.
    /// Populated by the `/add-dir <path>` slash command via the
    /// runtime's `session_additional_dirs` and threaded into every
    /// `ToolUseContext.permission_context.additional_dirs` so file/shell
    /// tools see the wider scope without persisting to settings.json.
    /// TS parity: `useWorkingDirectories` in REPL.tsx populates the
    /// same map from `/add-dir` invocations.
    pub session_additional_dirs:
        std::collections::HashMap<String, coco_types::AdditionalWorkingDir>,
    /// Working directory override for this session's tool calls.
    ///
    /// When `Some(path)`, [`ToolContextFactory`](crate::tool_context::ToolContextFactory)
    /// installs the path onto every `ToolUseContext.cwd_override` so
    /// file/shell/search tools that honor the override (Glob, Grep,
    /// Bash, and future worktree-aware tools) resolve relative paths
    /// against it. Absolute-path tools (Read, Write, Edit,
    /// NotebookEdit) are unaffected by construction — they enforce
    /// absolute paths in their schema, matching TS.
    ///
    /// Phase 6 Workstream C: subagents launched with
    /// `isolation: "worktree"` receive a `cwd_override` pointing at
    /// the freshly-created worktree path via this field on their
    /// child `QueryEngineConfig`.
    pub cwd_override: Option<std::path::PathBuf>,
    /// Optional override for the plans directory, relative to the
    /// project root. Empty = use the default `~/.cocode/plans/`.
    /// TS setting: `plansDirectory` in settings.json. Validated by
    /// [`coco_context::resolve_plans_directory`] to stay within the
    /// project root.
    pub plans_directory: Option<String>,
    /// Set when this engine runs AS a subagent — the agent's branded ID.
    /// Threads into `ToolUseContext::agent_id` + `session_plan_file` so
    /// the subagent auto-allow targets `{slug}-agent-{id}.md` instead of
    /// the main `{slug}.md`, and so the per-turn plan reminder picks the
    /// SubAgent text variant (TS: `isSubAgent` in `messages.ts:3399`).
    /// `None` = this engine IS the main session.
    pub agent_id: Option<String>,
    /// Set when this engine runs AS a swarm teammate (spawned via
    /// `TeamCreate` + in-process runner). TS: `isTeammate()` returns true
    /// when `agent_id.is_some() && team_name.is_some()` in the dynamic
    /// team context. We lift it to a config flag so `ToolUseContext.is_teammate`
    /// is set correctly without reading task-local state at every tool call.
    pub is_teammate: bool,
    /// Per-role `plan_mode_required` flag for teammates. TS:
    /// `isPlanModeRequired()` — read from the role definition in the
    /// team file or `COCO_PLAN_MODE_REQUIRED`. When `true`, the
    /// teammate's ExitPlanMode MUST request leader approval via mailbox.
    /// When `false`, teammates exit locally (voluntary plan mode).
    /// Only meaningful when `is_teammate == true`.
    pub plan_mode_required: bool,
    /// Plan-mode workflow + prompt settings. Drives which Full reminder
    /// variant `PlanModeReminder` emits. TS: `planModeV2.ts`.
    pub plan_mode_settings: PlanModeSettings,
    /// Disable all hooks (from settings).
    pub disable_all_hooks: bool,
    /// Only allow managed/policy hooks (from settings).
    pub allow_managed_hooks_only: bool,
    /// Enable token-budget-driven turn continuation: when a turn ends naturally
    /// (no tool calls, `end_turn` stop) but consumed tokens are below 90% of
    /// `max_tokens` budget, inject a nudge meta message and continue.
    /// TS: `query.ts:1308-1340` feature('TOKEN_BUDGET').
    pub enable_token_budget_continuation: bool,
    /// Resolved compaction configuration (auto / micro / api-native /
    /// session-memory / experimental). Single source of truth — engine
    /// reads only this, never env directly. TS env vars
    /// (`DISABLE_COMPACT`, `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`, …) are
    /// folded in by `coco_config::CompactConfig::resolve` at startup.
    pub compact: coco_config::CompactConfig,
    /// System-reminder subsystem configuration (per-generator toggles,
    /// timeout, critical-instruction payload). Bootstrap reads
    /// `settings.system_reminder` from `coco-config::Settings` and
    /// threads it through here so the engine can run `settings.json`
    /// through to every reminder generator without extra glue code.
    pub system_reminder: coco_config::SystemReminderConfig,
    /// Resolved tool runtime configuration.
    pub tool_config: ToolConfig,
    /// Resolved sandbox runtime configuration. User-facing settings only;
    /// for actual enforcement see [`Self::sandbox_state`].
    pub sandbox_config: SandboxSettings,
    /// Active sandbox runtime state. `None` when sandbox is disabled or
    /// not bootstrapped (test/headless paths). The CLI bootstrap layer
    /// constructs this via `coco_sandbox::adapter::build_runtime_config`
    /// and threads it onto `ToolUseContext.sandbox_state`.
    pub sandbox_state: Option<Arc<coco_sandbox::SandboxState>>,
    /// Resolved memory runtime configuration.
    pub memory_config: MemoryConfig,
    /// Resolved shell runtime configuration (bash-tool path).
    pub shell_config: ShellConfig,
    /// Session-scoped shell command assembler. Constructed once at
    /// session bootstrap (with the live snapshot watch + session-env
    /// reader + `/env` store) and threaded onto every
    /// [`coco_tool_runtime::ToolUseContext`]. `None` for tests / SDK
    /// paths that haven't yet wired the provider — Bash falls back to
    /// per-call executor construction without snapshot caching.
    pub shell_provider: Option<Arc<dyn coco_shell::ShellProvider>>,
    /// Frozen anchor — captured at session start. BashTool's
    /// `reset_cwd_if_outside_project` uses it to snap back when the
    /// live cwd drifts out of the allowed working set. TS:
    /// `bootstrap/state.ts::originalCwd`.
    pub original_cwd: Option<std::path::PathBuf>,
    /// Mutable session CWD shared across all BashTool invocations.
    /// `cd /tmp` in turn N updates this; turn N+1 reads it as the
    /// spawn cwd. TS parity: `bootstrap/state.ts::STATE.cwd` driven
    /// by `utils/Shell.ts::setCwd`. `None` for tests / SDK paths;
    /// BashTool falls back to `std::env::current_dir()`.
    pub session_cwd: Option<Arc<tokio::sync::RwLock<std::path::PathBuf>>>,
    /// Resolved web-fetch runtime configuration (WebFetchTool).
    pub web_fetch_config: WebFetchConfig,
    /// Resolved web-search runtime configuration (WebSearchTool).
    pub web_search_config: WebSearchConfig,
    /// Resolved LSP tool-layer runtime configuration (LspTool).
    /// Carries the per-query file-size gate; future fields land here.
    pub lsp_config: coco_config::LspConfig,
    /// Centralized feature gates (Layer 1 of the tool filter pipeline).
    /// See `docs/coco-rs/feature-gates-and-tool-filtering.md`.
    pub features: Arc<Features>,
    /// Layer 2 — extra tools the active model adds + baseline tools it
    /// excludes.
    pub tool_overrides: Arc<ToolOverrides>,
    /// Layer 4 — agent-level allow/deny list. Top-level sessions use
    /// `unrestricted()`; subagents narrow it from `AgentDefinition`.
    pub tool_filter: ToolFilter,
    /// Sandboxed write fence — FileWrite / FileEdit / NotebookEdit may
    /// only target paths under one of these roots. Empty = no fence.
    /// Set on subagents launched by the memory crate (extraction /
    /// auto-dream) so the child can only write inside the memdir.
    /// Threaded onto every `ToolUseContext.allowed_write_roots`.
    pub allowed_write_roots: Vec<std::path::PathBuf>,
    /// Emit `HookExecutionEvent` (`Started`/`Progress`/`Response`) into
    /// the SDK output stream. TS: `--include-hook-events` flag at
    /// `entrypoints/cli.tsx`. When `false`, the engine bypasses the
    /// hook-event forwarding channel so SDK clients don't receive
    /// `SDKHookStarted`/etc. messages. Defaults to `false` to match
    /// TS opt-in behaviour.
    pub include_hook_events: bool,
    /// Inter-turn reminder mailbox. Subsystems (slash commands, skill
    /// loader, tool runtime, swarm coordinator) push event-driven
    /// reminder snapshots into this — exposed to producers via
    /// [`Arc<dyn coco_system_reminder::ReminderMailboxRef>`] threaded
    /// onto [`coco_tool_runtime::ToolUseContext`]. The engine drains
    /// the **concrete** type (which exposes [`coco_system_reminder::ReminderMailbox::drain`])
    /// at the top of every turn so the next
    /// [`coco_system_reminder::TurnReminderInput`] picks up
    /// `command_permissions` / `dynamic_skill` / `structured_output` /
    /// `teammate_shutdown_batch` bodies. The trait split prevents
    /// producers from accidentally consuming the queue.
    pub reminder_mailbox: Arc<coco_system_reminder::ReminderMailbox>,

    /// Per-fork tool-execution gate. When `Some`, the engine threads
    /// the handle onto every `ToolUseContext` it builds, so the
    /// tool-call preparer runs the callback before the static
    /// permission evaluator. `None` preserves pre-canUseTool-wiring
    /// behavior. TS: `utils/forkedAgent.ts::runForkedAgent({canUseTool})`.
    pub can_use_tool: Option<coco_tool_runtime::CanUseToolHandleRef>,

    /// Override label returned by [`crate::engine_builder::query_source_label`].
    /// When `Some`, the engine reports this string instead of the
    /// agent_id / non-interactive / repl_main_thread default. Forks
    /// pass their `ForkedAgentOptions.query_source` through here so
    /// telemetry can split traffic per-fork. TS:
    /// `runForkedAgent({querySource})`.
    pub query_source_override: Option<String>,

    /// Typed fork discriminator. Threaded into
    /// [`crate::engine_session::run_internal_with_messages`]'s session-loop
    /// `info!` macro so log lines self-identify which fork they belong
    /// to. TS: `runForkedAgent({forkLabel})`.
    pub fork_label: Option<coco_types::ForkLabel>,

    /// Hard cap on output tokens, overriding the model's default.
    /// **WARNING**: setting this clamps `budget_tokens`, breaking
    /// prompt cache parity with the parent. PR #18143 incident:
    /// setting `effort: 'low'` on prompt-suggestion forks dropped
    /// cache hit rate from 92.7% → 61% (45× spike in cache writes).
    /// Only set this when cache parity is **not** a goal (e.g.
    /// compact summaries that intentionally use a different model
    /// and budget). The inference layer logs `tracing::warn!` when
    /// this field is `Some` so the regression leaves a trail.
    pub max_output_tokens_override: Option<i64>,

    /// Sub-context isolation overrides. `Some` ⇒ the engine is
    /// fork-spawned and the per-call `ToolUseContext` builder
    /// applies the [`ForkContextOverrides`] field-by-field
    /// (auto agent_id, query_chain_id / query_depth bump,
    /// allowed_write_roots fence, isolated callback handles).
    /// `None` ⇒ standard parent-shared semantics (default).
    /// TS parity: `forkedAgent.ts::createSubagentContext`.
    ///
    /// The `clone_file_read_state` flag inside is honored by the
    /// dispatcher at engine-build time (cloning is too expensive
    /// to repeat per-call); other flags apply at per-call
    /// `ToolUseContext` construction.
    ///
    /// Stored as `Arc` so threading it onto every per-call
    /// `ToolUseContext` is a cheap pointer-copy.
    pub fork_isolation: Option<std::sync::Arc<crate::fork_context::ForkContextOverrides>>,
}

impl Default for QueryEngineConfig {
    fn default() -> Self {
        Self {
            max_turns: 30,
            max_tokens: None,
            prompt_cache: None,
            system_prompt: None,
            append_system_prompt: None,
            model_id: String::new(),
            permission_mode: PermissionMode::Default,
            bypass_permissions_available: false,
            context_window: 200_000,
            max_output_tokens: 16_384,
            max_budget_usd: None,
            // Phase 9 landed: safe tools start mid-stream via
            // StreamingHandle, unsafe tools queue for commit_flush.
            // Default ON — the batched-at-end fallback path stays
            // available by setting this to `false`.
            streaming_tool_execution: true,
            is_non_interactive: false,
            debug: false,
            verbose: false,
            thinking_level: None,
            session_id: String::new(),
            project_dir: None,
            allow_rules: Default::default(),
            deny_rules: Default::default(),
            ask_rules: Default::default(),
            permission_rule_source_roots: Default::default(),
            session_additional_dirs: Default::default(),
            cwd_override: None,
            plans_directory: None,
            agent_id: None,
            is_teammate: false,
            plan_mode_required: false,
            plan_mode_settings: PlanModeSettings::default(),
            disable_all_hooks: false,
            allow_managed_hooks_only: false,
            enable_token_budget_continuation: false,
            compact: coco_config::CompactConfig::default(),
            system_reminder: coco_config::SystemReminderConfig::default(),
            tool_config: ToolConfig::default(),
            sandbox_config: SandboxSettings::default(),
            sandbox_state: None,
            memory_config: MemoryConfig::default(),
            shell_config: ShellConfig::default(),
            shell_provider: None,
            original_cwd: None,
            session_cwd: None,
            web_fetch_config: WebFetchConfig::default(),
            web_search_config: WebSearchConfig::default(),
            lsp_config: coco_config::LspConfig::default(),
            features: Arc::new(Features::with_defaults()),
            tool_overrides: Arc::new(ToolOverrides::none()),
            tool_filter: ToolFilter::unrestricted(),
            allowed_write_roots: Vec::new(),
            include_hook_events: false,
            reminder_mailbox: coco_system_reminder::ReminderMailbox::new(),
            can_use_tool: None,
            query_source_override: None,
            fork_label: None,
            max_output_tokens_override: None,
            fork_isolation: None,
        }
    }
}

impl QueryEngineConfig {
    /// Convenience: whether auto-compaction is currently allowed (user
    /// toggle AND env kill switches resolved). Used by the system-reminder
    /// generator and the auto-compact branch in `finalize_turn_post_tools`.
    #[must_use]
    pub fn is_auto_compact_active(&self) -> bool {
        self.compact.auto.is_active()
    }
}

/// One-shot bootstrap data for `SessionStarted` emission.
///
/// Collected by the CLI layer at session start and handed to the engine so it
/// can emit a single `CoreEvent::Protocol(ServerNotification::SessionStarted)`
/// with full context before the first turn.
///
/// TS equivalent: `buildSystemInitMessage()` in
/// `src/utils/messages/systemInit.ts`. Fields mirror
/// `SDKSystemMessageSchema` init subtype (coreSchemas.ts:1457-1494).
#[derive(Debug, Clone, Default)]
pub struct SessionBootstrap {
    pub protocol_version: String,
    pub cwd: String,
    pub version: String,
    /// Tool names the LLM will see. If empty, the engine falls back to
    /// `ToolRegistry::loaded_tools()` names.
    pub tools: Vec<String>,
    pub slash_commands: Vec<String>,
    pub agents: Vec<String>,
    pub skills: Vec<String>,
    pub mcp_servers: Vec<coco_types::McpServerInit>,
    pub plugins: Vec<coco_types::PluginInit>,
    pub api_key_source: Option<String>,
    pub betas: Vec<String>,
    pub output_style: Option<String>,
    pub fast_mode_state: Option<coco_types::FastModeState>,
}

/// Result of running the query engine.
#[derive(Debug)]
pub struct QueryResult {
    /// Final assistant text response.
    pub response_text: String,
    /// Total turns executed.
    pub turns: i32,
    /// Accumulated token usage.
    pub total_usage: TokenUsage,
    /// Per-model cost tracking.
    pub cost_tracker: CostTracker,
    /// Whether the engine was cancelled.
    pub cancelled: bool,
    /// Whether the budget was exhausted.
    pub budget_exhausted: bool,
    /// Why the engine stopped (last continue reason or None for clean exit).
    pub last_continue_reason: Option<ContinueReason>,
    /// Total duration in milliseconds.
    pub duration_ms: i64,
    /// Total API time in milliseconds.
    pub duration_api_ms: i64,
    /// Stop reason from the model.
    pub stop_reason: Option<String>,
    /// Permission denials accumulated during the session. Populated on each
    /// `PermissionDecision::Deny` branch in the tool execution loop and
    /// flushed into `SessionResultParams` at session end.
    /// Matches TS `SDKPermissionDenial` array (coreSchemas.ts:1399-1405).
    pub permission_denials: Vec<coco_types::PermissionDenialInfo>,
    /// Final message history at the end of the turn, including the
    /// user prompt, any tool calls + results, and the final assistant
    /// reply. Used by multi-turn SDK sessions to thread context
    /// forward into the next `turn/start`.
    pub final_messages: Vec<Message>,
}
