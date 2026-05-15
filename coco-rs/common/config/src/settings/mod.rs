pub mod merge;
pub mod policy;
pub mod source;
pub mod validation;
pub mod watcher;

use coco_types::PermissionMode;
use coco_types::ThinkingLevel;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::HashMap;

use crate::compact_settings::PartialCompactSettings;
use crate::model::ModelSelectionSettings;
use crate::prompt_cache_settings::PartialAccountSettings;
use crate::prompt_cache_settings::PartialPromptCacheSettings;
use crate::provider::PartialProviderConfig;
use crate::sandbox_settings::SandboxSettings;
use crate::sections::PartialApiSettings;
use crate::sections::PartialLoopSettings;
use crate::sections::PartialLspSettings;
use crate::sections::PartialMcpRuntimeSettings;
use crate::sections::PartialMemorySettings;
use crate::sections::PartialPathSettings;
use crate::sections::PartialShellSettings;
use crate::sections::PartialToolSettings;
use crate::sections::PartialWebFetchSettings;
use crate::sections::PartialWebSearchSettings;

pub use source::SettingSource;

pub const SYNTAX_HIGHLIGHTING_DISABLED_KEY: &str = "syntax_highlighting_disabled";

/// The merged settings snapshot. Immutable after loading.
/// TS: SettingsJson type in types.ts (Zod schema)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // === Auth ===
    /// Shell command that prints an API key on stdout. Consumed by
    /// `coco_inference::auth::resolve_auth` when env vars and stored
    /// tokens don't resolve.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_helper: Option<String>,

    // === Permissions ===
    pub permissions: PermissionsConfig,

    // === Model ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_models: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fast_mode: Option<bool>,
    /// JSON-first provider catalog overlays. Secrets should normally stay in
    /// provider `env_key` env vars rather than `api_key`. Each overlay is a
    /// `PartialProviderConfig` (every field `Option`), so unset fields leave
    /// the base catalog untouched. Identity is the parent map key — see
    /// `multi-provider-plan.md` §5.1.1.
    #[serde(default)]
    pub providers: BTreeMap<String, PartialProviderConfig>,
    #[serde(default)]
    pub models: ModelSelectionSettings,

    // === Environment ===
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Startup logging controls. Read by `app/cli` before installing
    /// the global tracing subscriber; env vars remain a higher-priority
    /// override layer.
    #[serde(default)]
    pub log: PartialLogSettings,

    // === Runtime components ===
    #[serde(default)]
    pub api: PartialApiSettings,
    #[serde(default, rename = "loop")]
    pub loop_config: PartialLoopSettings,
    #[serde(default)]
    pub tool: PartialToolSettings,
    #[serde(default)]
    pub shell: PartialShellSettings,
    #[serde(default)]
    pub sandbox: SandboxSettings,
    #[serde(default)]
    pub memory: PartialMemorySettings,
    #[serde(default, rename = "mcp")]
    pub mcp_runtime: PartialMcpRuntimeSettings,
    #[serde(default)]
    pub web_fetch: PartialWebFetchSettings,
    #[serde(default)]
    pub web_search: PartialWebSearchSettings,
    /// LSP tool-layer knobs. Resolved into `RuntimeConfig.lsp`
    /// (`LspConfig`); the file-size gate ships today, future fields
    /// (per-server overrides, prewarm policy) land in the same slot.
    /// Server roster lives in `~/.coco/lsp_servers.json` per the
    /// established `coco-lsp` design — not here.
    #[serde(default)]
    pub lsp: PartialLspSettings,
    #[serde(default)]
    pub paths: PartialPathSettings,

    // === Compaction ===
    /// Compaction (auto / micro / api-native / session-memory / experimental).
    /// Resolved at startup into `RuntimeConfig.compact` (`CompactConfig`);
    /// `coco_compact` reads only that struct, never env directly.
    #[serde(default)]
    pub compact: PartialCompactSettings,

    // === Prompt cache ===
    /// Provider-agnostic prompt-cache settings (currently the 1h-TTL
    /// allowlist). Resolved at startup into
    /// `RuntimeConfig.prompt_cache`. See `prompt-cache-design.md` §16a.
    #[serde(default)]
    pub prompt_cache: PartialPromptCacheSettings,

    // === Account / billing identity (Anthropic adapter consumes) ===
    /// User account identity (`api_key` / `claude_ai_subscriber`) +
    /// subscriber overage flag. Drives 1h-TTL eligibility latch + OAuth
    /// beta in the Anthropic adapter. **Session-stable** (R3-F3).
    #[serde(default)]
    pub account: PartialAccountSettings,

    // === Feature gates ===
    /// Coarse-grained feature toggles. Each key matches `Feature::key()`;
    /// unknown keys are silently ignored so old configs still load. See
    /// `docs/coco-rs/feature-gates-and-tool-filtering.md`.
    #[serde(default)]
    pub features: BTreeMap<String, bool>,

    // === Hooks ===
    /// Deserialized by coco-hooks, kept as Value here (avoids L1→L4 dep).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<serde_json::Value>,
    #[serde(default)]
    pub disable_all_hooks: bool,

    // === MCP ===
    #[serde(default)]
    pub allowed_mcp_servers: Vec<AllowedMcpServerEntry>,
    #[serde(default)]
    pub denied_mcp_servers: Vec<DeniedMcpServerEntry>,
    #[serde(default)]
    pub enable_all_project_mcp_servers: bool,

    // === Display ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default)]
    pub syntax_highlighting_disabled: bool,

    // === Plugins ===
    #[serde(default)]
    pub enabled_plugins: HashMap<String, PluginConfig>,

    // === Worktree ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorktreeConfig>,

    // === Plans ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plans_directory: Option<String>,

    // === Plan mode ===
    /// Plan-mode workflow + Phase-4 prompt variant + per-phase agent
    /// counts. Port of TS `planModeV2.ts` behaviors:
    /// `isPlanModeInterviewPhaseEnabled`, `getPewterLedgerVariant`,
    /// `getPlanModeV2AgentCount`, `getPlanModeV2ExploreAgentCount` —
    /// but re-rooted on user-visible config instead of GrowthBook /
    /// USER_TYPE=ant gating. See root `CLAUDE.md` "Plan Mode — Skip
    /// Ultraplan" decision row.
    #[serde(default)]
    pub plan_mode: PlanModeSettings,

    // === System reminder ===
    /// Per-reminder enable flags + orchestrator timeout + user-supplied
    /// critical instruction. Consumed by `coco-system-reminder` via a
    /// `coco-config` dependency; there is no parallel config struct.
    #[serde(default)]
    pub system_reminder: crate::system_reminder::SystemReminderConfig,

    // === Session ===
    /// Session-level behaviors: auto-title generation, etc.
    #[serde(default)]
    pub session: SessionSettings,

    // === Auto-Mode ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_mode: Option<AutoModeConfig>,

    // === Policy ===
    /// When true, only managed (policy-level) hooks are allowed to run.
    #[serde(default)]
    pub allow_managed_hooks_only: bool,
    /// When true, only plugin-level customization is permitted.
    #[serde(default)]
    pub strict_plugin_only_customization: bool,

    // === File Checkpointing ===
    /// When false, disables file checkpointing for rewind.
    /// TS: `fileCheckpointingEnabled` in supportedSettings.ts
    #[serde(default = "default_true")]
    pub file_checkpointing_enabled: bool,

    // === Attribution ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_co_authored_by: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_git_instructions: Option<bool>,
}

/// Permission rules configuration within settings.
///
/// Rules are stored on disk as string arrays matching the TS format:
/// `{ "permissions": { "allow": ["Bash", "Bash(git *)"], "deny": [...] } }`
///
/// Use `coco_permissions::parse_rule_string()` to convert these strings
/// into typed `PermissionRule` values at load time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionsConfig {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub ask: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_mode: Option<PermissionMode>,
    pub disable_bypass_mode: bool,
    #[serde(default)]
    pub additional_directories: Vec<String>,
    /// When true, only rules from policy settings are respected.
    /// TS: allowManagedPermissionRulesOnly
    #[serde(default)]
    pub allow_managed_permission_rules_only: bool,
}

/// Optional `settings.json` logging block consumed at process startup.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PartialLogSettings {
    /// Tracing filter directive. `filter` is accepted as an alias because
    /// the resolved value is reported as `log_filter` in startup logs.
    #[serde(
        alias = "filter",
        alias = "log_filter",
        skip_serializing_if = "Option::is_none"
    )]
    pub level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

/// Auto-mode/yolo classifier user configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoModeConfig {
    pub allow: Vec<String>,
    pub soft_deny: Vec<String>,
    pub environment: Vec<String>,
}

/// An allowed MCP server entry in settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllowedMcpServerEntry {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

/// A denied MCP server entry in settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeniedMcpServerEntry {
    pub name: String,
}

/// Plugin configuration in settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginConfig {
    pub enabled: bool,
}

/// Worktree configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorktreeConfig {
    pub enabled: bool,
}

/// Plan-mode workflow + prompt configuration.
///
/// TS parity (re-rooted on user config, not GrowthBook):
/// - `workflow` ← `isPlanModeInterviewPhaseEnabled`
/// - `phase4_variant` ← `getPewterLedgerVariant`
/// - `explore_agent_count` ← `getPlanModeV2ExploreAgentCount`
/// - `plan_agent_count` ← `getPlanModeV2AgentCount`
///
/// All fields have sensible defaults so users who don't touch their
/// settings.json get the canonical 5-phase workflow + standard Phase 4.
///
/// `Default` is implemented manually (not derived) because the field-level
/// `#[serde(default = "...")]` annotations do NOT participate in
/// `#[derive(Default)]`. A `derive(Default)` instance would silently zero
/// `explore_agent_count` / `plan_agent_count` /
/// `plan_model_fallback_threshold_tokens` — which is wrong for the
/// "user has no `plan_mode` block in settings.json" path (outer
/// `#[serde(default)]` on the parent struct uses `Default`, not the
/// field-level fns). Manual `Default` mirrors the field-level fns so
/// every construction path produces the same sensible values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PlanModeSettings {
    /// Which workflow the Full plan-mode reminder should emit.
    pub workflow: PlanModeWorkflow,
    /// Phase-4 prompt variant (five-phase workflow only).
    /// Ignored when `workflow = interview`.
    pub phase4_variant: PlanPhase4Variant,
    /// How many parallel Explore agents the 5-phase prompt invites the
    /// model to launch. TS default: 3. Valid range [1, 10].
    #[serde(default = "default_explore_agent_count")]
    pub explore_agent_count: i32,
    /// How many parallel Plan agents the 5-phase prompt invites. TS
    /// default: 1 (3 for Max/enterprise/team tiers in TS, but we don't
    /// ship tier detection — user picks). Valid range [1, 10].
    #[serde(default = "default_plan_agent_count")]
    pub plan_agent_count: i32,
    /// Advisory mtime check on plan-mode exit: compare the plan file's
    /// mtime against the `EnterPlanMode` entry timestamp. On
    /// `NotEdited` / `Missing`, append a non-blocking advisory note to
    /// the `ExitPlanMode` tool_result. **Does not enforce** — the model
    /// can ignore the advisory. Default off.
    ///
    /// TS parity: older hook paths were refactored into the
    /// `VerifyPlanExecution` tool. This Rust setting remains the
    /// simpler synchronous mtime check on `ExitPlanMode`; it is advisory
    /// and does not perform post-implementation verification. Name kept
    /// as `verify_execution` for settings.json backwards compatibility.
    #[serde(default)]
    pub verify_execution: bool,
    /// In plan mode, if the latest assistant message's context exceeds
    /// this token count, the engine falls back from the configured
    /// `models.plan` client to the `models.main` client to avoid
    /// truncation.
    ///
    /// TS parity: `getRuntimeMainLoopModel`'s `exceeds200kTokens` branch
    /// (utils/model/model.ts:152-159). TS hardcodes 200_000 as the
    /// threshold; coco-rs exposes it so multi-LLM users can tune for
    /// their plan-role model's actual context window.
    ///
    /// Default 200_000. Set to `i64::MAX` to disable fallback; set to 0
    /// to always fall back (effectively disabling plan-mode model swap).
    #[serde(default = "default_plan_model_fallback_threshold")]
    pub plan_model_fallback_threshold_tokens: i64,
    /// Whether the `ExitPlanMode` permission dialog offers a "clear
    /// context" option in addition to the default yes/no choice.
    ///
    /// TS parity: `settings.showClearContextOnPlanAccept`
    /// (utils/settings/types.ts:735-740), default false. When true the
    /// TUI surfaces keep/clear/cancel choices; selecting clear schedules
    /// `MessageHistory::clear()` at the next turn boundary.
    #[serde(default)]
    pub show_clear_context_on_exit: bool,
}

fn default_explore_agent_count() -> i32 {
    3
}
fn default_plan_agent_count() -> i32 {
    1
}
fn default_plan_model_fallback_threshold() -> i64 {
    200_000
}

impl Default for PlanModeSettings {
    fn default() -> Self {
        Self {
            workflow: PlanModeWorkflow::default(),
            phase4_variant: PlanPhase4Variant::default(),
            explore_agent_count: default_explore_agent_count(),
            plan_agent_count: default_plan_agent_count(),
            verify_execution: false,
            plan_model_fallback_threshold_tokens: default_plan_model_fallback_threshold(),
            show_clear_context_on_exit: false,
        }
    }
}

/// The plan-mode Full reminder workflow variant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanModeWorkflow {
    /// Original 5-phase workflow: Understand → Explore → Design → Final
    /// Plan → ExitPlanMode. Heavy agent parallelism in Phase 1 + 2.
    /// TS: `getPlanModeV2MainAgentInstructions`.
    #[default]
    FivePhase,
    /// Iterative ask-as-you-go workflow: read a little, ask, update the
    /// plan file, repeat. TS: `getPlanModeInterviewInstructions`.
    Interview,
}

/// Session-level auto-behavior configuration.
///
/// Toggles for features that run across the session lifecycle, not tied
/// to any single prompt or turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionSettings {
    /// Auto-generate a session title from the approved plan text the
    /// first time `ExitPlanMode` is approved and a plan file is
    /// non-empty. Uses whatever provider+model is currently bound to
    /// `ModelRole::Fast` — if no Fast role is configured, the feature
    /// silently stays off. TS: `sessionTitle.ts::generateSessionTitle`
    /// + `initReplBridge.ts::onUserMessage` lifecycle hook.
    pub auto_title: bool,
}

/// Phase-4 "Final Plan" prompt strictness (5-phase workflow only).
/// TS: `PewterLedgerVariant` — four arms with progressively stricter
/// guidance on plan-file length.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanPhase4Variant {
    /// Standard detailed plan with Context + Verification sections.
    #[default]
    Standard,
    /// One-line Context, single verification command.
    Trim,
    /// No Context / Background; one line per file. Soft 40-line guidance.
    Cut,
    /// Hardest: no prose, bullet per file, **hard 40-line limit**.
    Cap,
}

/// Settings snapshot with per-source tracking.
#[derive(Debug, Clone)]
pub struct SettingsWithSource {
    pub merged: Settings,
    pub per_source: HashMap<SettingSource, serde_json::Value>,
}

fn default_true() -> bool {
    true
}

/// Load settings from a JSON string.
pub fn parse_settings(json: &str) -> crate::Result<Settings> {
    let settings: Settings = crate::jsonc::from_str(json)?;
    Ok(settings)
}

/// Load and merge settings using the default user / managed paths
/// (`~/.coco/settings.json` and the platform-managed file).
pub fn load_settings(
    cwd: &std::path::Path,
    flag_settings: Option<&std::path::Path>,
) -> crate::Result<SettingsWithSource> {
    load_settings_with(
        cwd,
        flag_settings,
        &crate::global_config::user_settings_path(),
        &crate::global_config::managed_settings_path(),
    )
}

/// Load and merge settings with explicit user / managed paths.
/// Tests pass TempDir-rooted paths to isolate from the developer's
/// real `~/.coco/`.
///
/// Merge order (later overrides earlier):
///   1. Plugin base
///   2. User global (`user_path`)
///   3. Project shared (`{cwd}/.claude/settings.json`)
///   4. Project local (`{cwd}/.claude/settings.local.json`)
///   5. Flag (`--settings file`)
///   6. Policy (`managed_path`)
pub fn load_settings_with(
    cwd: &std::path::Path,
    flag_settings: Option<&std::path::Path>,
    user_path: &std::path::Path,
    managed_path: &std::path::Path,
) -> crate::Result<SettingsWithSource> {
    use crate::ResultExt;
    use crate::global_config;

    let mut per_source = HashMap::new();
    let mut merged = serde_json::Value::Object(serde_json::Map::new());

    let user_pathbuf = user_path.to_path_buf();
    let sources = [
        (SettingSource::User, user_pathbuf),
        (
            SettingSource::Project,
            global_config::project_settings_path(cwd),
        ),
        (
            SettingSource::Local,
            global_config::local_settings_path(cwd),
        ),
    ];

    for (source, path) in &sources {
        if path.exists() {
            load_and_merge(&mut per_source, &mut merged, *source, path)?;
        }
    }

    // Flag settings (`--settings <path>`).
    if let Some(flag_path) = flag_settings
        && flag_path.exists()
    {
        load_and_merge(&mut per_source, &mut merged, SettingSource::Flag, flag_path)?;
    }

    // Policy / managed settings (highest precedence).
    if managed_path.exists() {
        load_and_merge(
            &mut per_source,
            &mut merged,
            SettingSource::Policy,
            managed_path,
        )?;
    }

    let settings: Settings = serde_json::from_value(merged)
        .with_ctx("failed to deserialize merged settings into Settings struct")?;

    Ok(SettingsWithSource {
        merged: settings,
        per_source,
    })
}

/// Read + parse a settings layer, propagate IO / parse errors with
/// the offending path attached. Silently swallowing these used to
/// confuse users whose edits had no observable effect — now the
/// CLI fails fast at startup with a clear error.
fn load_and_merge(
    per_source: &mut HashMap<SettingSource, serde_json::Value>,
    merged: &mut serde_json::Value,
    source: SettingSource,
    path: &std::path::Path,
) -> crate::Result<()> {
    use crate::ResultExt;
    let contents = std::fs::read_to_string(path)
        .with_ctx_lazy(|| format!("failed to read settings file: {}", path.display()))?;
    let value = crate::jsonc::parse_value(&contents)
        .with_ctx_lazy(|| format!("failed to parse JSONC in settings file: {}", path.display()))?;
    per_source.insert(source, value.clone());
    merge::deep_merge(merged, &value);
    Ok(())
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
