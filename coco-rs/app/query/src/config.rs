//! Configuration + result types for `QueryEngine`.
//!
//! Extracted from `engine.rs` so the orchestration module stays focused on the
//! session loop. Pure data types — no behavior lives here.

use coco_config::MemoryConfig;
use coco_config::PlanModeSettings;
use coco_config::SandboxConfig;
use coco_config::ShellConfig;
use coco_config::ToolConfig;
use coco_config::WebFetchConfig;
use coco_config::WebSearchConfig;
use coco_messages::CostTracker;
use coco_types::Message;
use coco_types::PermissionMode;
use coco_types::TokenUsage;

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
    /// System prompt to prepend.
    pub system_prompt: Option<String>,
    /// Append to system prompt (after CLAUDE.md).
    pub append_system_prompt: Option<String>,
    /// Model name for tool context.
    pub model_name: String,
    /// Fallback model for error recovery.
    pub fallback_model: Option<String>,
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
    /// Session identifier for hook orchestration context.
    pub session_id: String,
    /// Project root directory for hook orchestration context.
    pub project_dir: Option<std::path::PathBuf>,
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
    /// User preference for auto-compaction. When false, the
    /// `compaction_reminder` system-reminder is suppressed and
    /// `services/compact::should_auto_compact` is bypassed. TS:
    /// `isAutoCompactEnabled()` — mapped to `settings.json` in coco-rs.
    /// Defaults to `true` to match TS default behavior.
    pub auto_compact_enabled: bool,
    /// System-reminder subsystem configuration (per-generator toggles,
    /// timeout, critical-instruction payload). Bootstrap reads
    /// `settings.system_reminder` from `coco-config::Settings` and
    /// threads it through here so the engine can run `settings.json`
    /// through to every reminder generator without extra glue code.
    pub system_reminder: coco_config::SystemReminderConfig,
    /// Resolved tool runtime configuration.
    pub tool_config: ToolConfig,
    /// Resolved sandbox runtime configuration.
    pub sandbox_config: SandboxConfig,
    /// Resolved memory runtime configuration.
    pub memory_config: MemoryConfig,
    /// Resolved shell runtime configuration (bash-tool path).
    pub shell_config: ShellConfig,
    /// Resolved web-fetch runtime configuration (WebFetchTool).
    pub web_fetch_config: WebFetchConfig,
    /// Resolved web-search runtime configuration (WebSearchTool).
    pub web_search_config: WebSearchConfig,
}

impl Default for QueryEngineConfig {
    fn default() -> Self {
        Self {
            max_turns: 30,
            max_tokens: None,
            system_prompt: None,
            append_system_prompt: None,
            model_name: String::new(),
            fallback_model: None,
            permission_mode: PermissionMode::Default,
            bypass_permissions_available: false,
            context_window: 200_000,
            max_output_tokens: 16_384,
            max_budget_usd: None,
            streaming_tool_execution: true,
            is_non_interactive: false,
            session_id: String::new(),
            project_dir: None,
            plans_directory: None,
            agent_id: None,
            is_teammate: false,
            plan_mode_required: false,
            plan_mode_settings: PlanModeSettings::default(),
            disable_all_hooks: false,
            allow_managed_hooks_only: false,
            enable_token_budget_continuation: false,
            auto_compact_enabled: true,
            system_reminder: coco_config::SystemReminderConfig::default(),
            tool_config: ToolConfig::default(),
            sandbox_config: SandboxConfig::default(),
            memory_config: MemoryConfig::default(),
            shell_config: ShellConfig::default(),
            web_fetch_config: WebFetchConfig::default(),
            web_search_config: WebSearchConfig::default(),
        }
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
