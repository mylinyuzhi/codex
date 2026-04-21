pub mod merge;
pub mod policy;
pub mod source;
pub mod validation;
pub mod watcher;

use coco_types::PermissionMode;
use coco_types::ThinkingLevel;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

use crate::model::ModelSelectionSettings;
use crate::provider::ProviderConfig;
use crate::sections::PartialApiSettings;
use crate::sections::PartialLoopSettings;
use crate::sections::PartialMcpRuntimeSettings;
use crate::sections::PartialMemorySettings;
use crate::sections::PartialPathSettings;
use crate::sections::PartialSandboxSettings;
use crate::sections::PartialShellSettings;
use crate::sections::PartialToolSettings;
use crate::sections::PartialWebFetchSettings;
use crate::sections::PartialWebSearchSettings;

pub use source::SettingSource;

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
    /// JSON-first provider catalog overrides. Secrets should normally stay in
    /// provider `env_key` env vars rather than `api_key`.
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub models: ModelSelectionSettings,

    // === Environment ===
    #[serde(default)]
    pub env: HashMap<String, String>,

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
    pub sandbox: PartialSandboxSettings,
    #[serde(default)]
    pub memory: PartialMemorySettings,
    #[serde(default, rename = "mcp")]
    pub mcp_runtime: PartialMcpRuntimeSettings,
    #[serde(default)]
    pub web_fetch: PartialWebFetchSettings,
    #[serde(default)]
    pub web_search: PartialWebSearchSettings,
    #[serde(default)]
    pub paths: PartialPathSettings,

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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// TS parity: `VerifyPlanExecution` is a PostToolUse *hook* in TS
    /// that can block the exit. The Rust port ships the simpler
    /// synchronous mtime check as an advisory; if enforcement is
    /// needed, wire a hook instead. Name kept as `verify_execution` for
    /// settings.json backwards compatibility.
    #[serde(default)]
    pub verify_execution: bool,
}

fn default_explore_agent_count() -> i32 {
    3
}
fn default_plan_agent_count() -> i32 {
    1
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
pub fn parse_settings(json: &str) -> anyhow::Result<Settings> {
    let settings: Settings = serde_json::from_str(json)?;
    Ok(settings)
}

/// Load and merge settings from multiple sources.
/// Merge order (later overrides earlier):
///   1. Plugin base
///   2. User global (~/.coco/settings.json)
///   3. Project shared (.claude/settings.json)
///   4. Project local (.claude/settings.local.json)
///   5. Flag (--settings file)
///   6. Policy (enterprise managed)
pub fn load_settings(
    cwd: &std::path::Path,
    flag_settings: Option<&std::path::Path>,
) -> anyhow::Result<SettingsWithSource> {
    use crate::global_config;

    let mut per_source = HashMap::new();
    let mut merged = serde_json::Value::Object(serde_json::Map::new());

    // Load each source if it exists, merge in order
    let sources = [
        (SettingSource::User, global_config::user_settings_path()),
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
        if path.exists()
            && let Ok(contents) = std::fs::read_to_string(path)
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents)
        {
            per_source.insert(*source, value.clone());
            merge::deep_merge(&mut merged, &value);
        }
    }

    // Flag settings
    if let Some(flag_path) = flag_settings
        && flag_path.exists()
        && let Ok(contents) = std::fs::read_to_string(flag_path)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents)
    {
        per_source.insert(SettingSource::Flag, value.clone());
        merge::deep_merge(&mut merged, &value);
    }

    // Policy settings
    let policy_path = global_config::managed_settings_path();
    if policy_path.exists()
        && let Ok(contents) = std::fs::read_to_string(&policy_path)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents)
    {
        per_source.insert(SettingSource::Policy, value.clone());
        merge::deep_merge(&mut merged, &value);
    }

    let settings: Settings = serde_json::from_value(merged)?;

    Ok(SettingsWithSource {
        merged: settings,
        per_source,
    })
}
