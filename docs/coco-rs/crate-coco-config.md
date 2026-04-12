# coco-config — Crate Plan

TS source: `src/utils/settings/`, `src/utils/model/`, `src/utils/effort.ts`, `src/utils/fastMode.ts`, `src/constants/`, `src/migrations/`, `src/utils/envUtils.ts`, `src/utils/config.ts`

## Scope

coco-config owns ONLY:
- `~/.coco.json` (GlobalConfig)
- `~/.coco/settings.json` (user settings)
- `.claude/settings.json` (project settings)
- `.claude/settings.local.json` (local settings)
- Managed/enterprise settings
- `~/.coco/cache/` (model capabilities cache)
- Model selection, provider config, effort, fast mode

coco-config does NOT own: CLAUDE.md (coco-context), .mcp.json (coco-mcp), skills/ (coco-skills), etc.
See `config-file-map.md` for the full ownership map.

## Dependencies

```
coco-config depends on:
  - coco-types           (PermissionMode, ModelRole, ModelSpec, ProviderApi, Capability, etc.)
  - coco-error           (ConfigError)
  - utils/common         (find_coco_home(), LoggingConfig — home dir discovery, REUSE)
  - utils/file-watch     (FileWatcher, FileWatcherBuilder — config file watching, REUSE)
  - utils/absolute-path  (AbsolutePathBuf, ~ expansion — path normalization, REUSE)
  - serde, serde_json    (deserialization)

coco-config does NOT depend on:
  - notify directly (uses utils/file-watch wrapper instead)
  - dirs directly (uses utils/common home dir discovery instead)
  - coco-inference, coco-tool, or any app/ crate
```

## Modules

```
coco-config/src/
  lib.rs
  global_config.rs      # GlobalConfig (~/.coco.json) — user ID, theme, auth, projects
  settings/
    mod.rs              # Settings struct, SettingsWithSource
    schema.rs           # Zod-equivalent serde validation
    loader.rs           # Layered loading: plugin < user < project < local < flag < policy
    watcher.rs          # File watcher via utils/file-watch (debounce, coalesce)
    source.rs           # SettingSource enum, source tracking
    merge.rs            # Deep merge with array dedup
    managed.rs          # Enterprise/MDM policy loading
    validation.rs       # Validation errors, unknown field preservation
    migration.rs        # Settings format migrations
  provider/
    mod.rs              # ProviderConfig, per-provider env_key resolution
    builtin.rs          # Built-in provider definitions (Anthropic, OpenAI, Google, etc.)
  model/
    mod.rs              # get_main_loop_model(), model selection
    configs.rs          # ALL_MODEL_CONFIGS (per-provider model IDs)
    aliases.rs          # ModelAlias resolution
    capabilities.rs     # ModelInfo, Capability checks
    agent.rs            # Subagent model selection
  effort.rs             # ThinkingLevel support checks (TS effort.ts)
  fast_mode.rs          # FastModeState, cooldown
  env.rs                # Environment variable helpers (is_env_truthy, etc.)
  constants.rs          # System constants
```

---

## 0. GlobalConfig (~/.coco.json) — TS loads this separately

TS loads `~/.claude.json` (via `getGlobalConfig()` in `utils/config.ts`). This is **NOT** settings.json — it's a separate file for user-level state.

```rust
/// Per-user global config. Separate from Settings.
/// TS: GlobalConfig type in utils/config.ts, stored at ~/.claude.json
/// Rust: stored at ~/.coco.json (or COCO_CONFIG_DIR/.coco.json)
///
/// Uses utils/common::find_coco_home() for home dir discovery.
#[derive(Serialize, Deserialize, Default)]
pub struct GlobalConfig {
    pub user_id: Option<String>,
    pub theme: Option<String>,
    pub projects: HashMap<String, ProjectConfig>,  // keyed by project path hash
    pub session_costs: HashMap<String, SessionCostState>,
    pub companion: Option<CompanionConfig>,         // buddy pet state
    // ... auth tokens, onboarding state, etc.
}

/// Per-project config within GlobalConfig.
pub struct ProjectConfig {
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub costs: Option<SessionCostState>,
}

/// Load with file locking (prevents corruption from concurrent processes).
/// TS: uses lockfile.ts for atomic reads/writes.
pub fn load_global_config() -> Result<GlobalConfig, ConfigError>;
pub fn write_global_config(config: &GlobalConfig) -> Result<(), ConfigError>;
```

### File paths (using utils/common + utils/absolute-path)

```rust
/// Uses utils/common::find_coco_home() — respects COCO_CONFIG_DIR env
pub fn global_config_path() -> AbsolutePathBuf {
    find_coco_home().join(".coco.json")  // ~/.coco.json
}
pub fn config_home() -> AbsolutePathBuf {
    find_coco_home()  // ~/.coco/ (or COCO_CONFIG_DIR)
}
```

---

## 1. Settings Loading Architecture (TS-first)

### TS design: Layered merge with source tracking

TS loads settings from 6 sources, merging later over earlier. Each source is a JSON file. Env vars are a **separate** override layer (not in the merged settings).

### SettingSource enum

```rust
/// Where a setting came from. Used for conflict resolution and security.
/// TS: SettingSource type in settings.ts
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SettingSource {
    Plugin,     // Plugin-contributed base settings (lowest priority)
    User,       // ~/.coco/settings.json (global per machine)
    Project,    // .claude/settings.json (shared, checked in)
    Local,      // .claude/settings.local.json (gitignored)
    Flag,       // --settings CLI file or SDK inline
    Policy,     // Enterprise/MDM managed (highest priority)
}
```

### Settings struct

```rust
/// The merged settings snapshot. Immutable after loading.
/// TS: SettingsJson type in types.ts (Zod schema)
#[derive(Deserialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]  // strict validation
pub struct Settings {
    // === Auth ===
    pub api_key_helper: Option<String>,
    pub force_login_method: Option<LoginMethod>,

    // === Permissions ===
    pub permissions: PermissionsConfig,

    // === Model ===
    pub model: Option<String>,                              // user-specified model
    pub available_models: Option<Vec<String>>,              // enterprise allowlist
    pub model_overrides: Option<HashMap<String, String>>,   // canonical -> provider-specific
    pub thinking_level: Option<ThinkingLevel>,
    pub fast_mode: Option<bool>,
    pub always_thinking_enabled: Option<bool>,

    // === Environment ===
    pub env: HashMap<String, String>,   // injected into shell execution

    // === Hooks ===
    pub hooks: Option<Value>,  // deserialized by coco-hooks, not typed here (avoids L1->L4 dep)
    pub disable_all_hooks: bool,

    // === MCP ===
    pub allowed_mcp_servers: Vec<AllowedMcpServerEntry>,
    pub denied_mcp_servers: Vec<DeniedMcpServerEntry>,
    pub enable_all_project_mcp_servers: bool,

    // === Shell ===
    pub default_shell: Option<ShellKind>,   // Bash (default), PowerShell — defined in coco-context

    // === Display ===
    pub output_style: Option<String>,
    pub language: Option<String>,
    pub syntax_highlighting_disabled: bool,

    // === Plugins ===
    pub enabled_plugins: HashMap<String, PluginConfig>,

    // === Worktree ===
    pub worktree: Option<WorktreeConfig>,

    // === Plans ===
    pub plans_directory: Option<String>,

    // === Auto-Mode ===
    pub auto_mode: Option<AutoModeConfig>,

    // === Attribution ===
    pub include_co_authored_by: Option<bool>,
    pub include_git_instructions: Option<bool>,
}

/// Auto-mode/yolo classifier user configuration.
/// Read by coco-permissions for classifier prompt construction.
#[derive(Deserialize, Default, Clone)]
pub struct AutoModeConfig {
    pub allow: Vec<String>,       // Safe patterns (e.g., "git commits")
    pub soft_deny: Vec<String>,   // Suspicious patterns (e.g., "file deletion")
    pub environment: Vec<String>, // Context (e.g., "building web app")
}

#[derive(Deserialize, Default, Clone)]
pub struct PermissionsConfig {
    pub allow: Vec<PermissionRule>,
    pub deny: Vec<PermissionRule>,
    pub ask: Vec<PermissionRule>,
    pub default_mode: Option<PermissionMode>,
    pub disable_bypass_mode: bool,
    pub additional_directories: Vec<String>,
}
```

### SettingsWithSource (tracks which source provided each field)

```rust
/// Settings snapshot with per-field source tracking.
/// Used for security (project settings can't set certain fields).
pub struct SettingsWithSource {
    pub merged: Settings,
    pub per_source: HashMap<SettingSource, Settings>,  // raw per-source before merge
}

impl SettingsWithSource {
    /// Check if a field was set by a specific source.
    pub fn source_of(&self, field: &str) -> Option<SettingSource> { ... }
}
```

---

## 2. Settings Loading Chain

```rust
/// TS: loadSettingsFromDisk() in settings.ts
/// Merge order (later overrides earlier):
///   1. Plugin base (contributed by enabled plugins)
///   2. User global (~/.coco/settings.json)
///   3. Project shared (.claude/settings.json)
///   4. Project local (.claude/settings.local.json, gitignored)
///   5. Flag (--settings file or SDK inline)
///   6. Policy (enterprise managed, highest priority)
///
/// Merge algorithm: deep merge, arrays concatenated + deduped.
pub fn load_settings(cwd: &Path, flag_settings: Option<&Path>) -> Result<SettingsWithSource, ConfigError>;
```

### File paths (reuses utils/common + utils/absolute-path)

```rust
use coco_utils_common::find_coco_home;    // REUSE: home dir discovery
use coco_utils_absolute_path::AbsolutePathBuf;  // REUSE: path normalization

/// TS: getClaudeConfigHomeDir()
/// Uses utils/common::find_coco_home() — respects COCO_CONFIG_DIR env.
/// DO NOT reimplement home dir logic — utils/common already handles this.
pub fn config_home() -> AbsolutePathBuf {
    find_coco_home()  // ~/.coco or $COCO_CONFIG_DIR
}

pub fn user_settings_path() -> AbsolutePathBuf { config_home().join("settings.json") }
pub fn project_settings_path(cwd: &Path) -> AbsolutePathBuf {
    AbsolutePathBuf::new(cwd.join(".claude/settings.json"))
}
pub fn local_settings_path(cwd: &Path) -> AbsolutePathBuf {
    AbsolutePathBuf::new(cwd.join(".claude/settings.local.json"))
}

/// Enterprise/MDM managed settings (platform-specific)
/// TS: managedPath.ts
pub fn managed_settings_path() -> AbsolutePathBuf {
    #[cfg(target_os = "macos")]
    { AbsolutePathBuf::new("/Library/Application Support/CoCo/managed-settings.json") }
    #[cfg(target_os = "linux")]
    { AbsolutePathBuf::new("/etc/coco/managed-settings.json") }
    #[cfg(target_os = "windows")]
    { AbsolutePathBuf::new(r"C:\Program Files\CoCo\managed-settings.json") }
}
/// Plus managed-settings.d/*.json (alphabetically sorted drop-in fragments)
```

### Policy loading (enterprise)

```rust
/// TS: "first source wins" — highest-priority source provides ALL policy settings.
/// Sources in order: remote > MDM/plist/HKLM > file > HKCU
pub fn load_policy_settings() -> Option<Settings> {
    // 1. Try remote managed (cached from API sync)
    // 2. Try OS-level MDM (macOS plist / Windows HKLM)
    // 3. Try file-based managed-settings.json + .d/
    // Return first non-empty source
}
```

---

## 3. Environment Variable Design

### Pattern: env vars are a separate override layer, NOT inside Settings struct

TS does not merge env vars into the Settings struct. Instead, each subsystem checks both config and env at resolution time. Rust follows the same pattern.

### Env-only variables (no config file equivalent)

```rust
/// Env-only config. No Settings file equivalent.
/// TS is Anthropic-centric; Rust extends for multi-provider.
pub struct EnvOnlyConfig {
    // === Anthropic deployment routing (from TS) ===
    // These select WHICH Anthropic endpoint, not which provider.
    pub use_bedrock: bool,           // CLAUDE_CODE_USE_BEDROCK (AWS)
    pub use_vertex: bool,            // CLAUDE_CODE_USE_VERTEX (GCP)
    pub use_foundry: bool,           // CLAUDE_CODE_USE_FOUNDRY (Azure)

    // === Model override (higher priority than settings.model) ===
    pub model_override: Option<String>,  // ANTHROPIC_MODEL (or COCO_MODEL for multi-provider)
    pub small_fast_model: Option<String>,// ANTHROPIC_SMALL_FAST_MODEL
    pub subagent_model: Option<String>,  // CLAUDE_CODE_SUBAGENT_MODEL

    // === Shell ===
    pub shell_override: Option<String>,  // CLAUDE_CODE_SHELL

    // === Limits ===
    pub max_tool_concurrency: Option<i32>,  // CLAUDE_CODE_MAX_TOOL_USE_CONCURRENCY
    pub max_context_tokens: Option<i64>,    // CLAUDE_CODE_MAX_CONTEXT_TOKENS

    // === Runtime flags ===
    pub bare_mode: bool,              // CLAUDE_CODE_SIMPLE=1 or --bare
}

impl EnvOnlyConfig {
    /// Read all env vars once at startup. Uses utils/env helpers.
    pub fn from_env() -> Self {
        Self {
            use_bedrock: is_env_truthy("CLAUDE_CODE_USE_BEDROCK"),
            use_vertex: is_env_truthy("CLAUDE_CODE_USE_VERTEX"),
            use_foundry: is_env_truthy("CLAUDE_CODE_USE_FOUNDRY"),
            model_override: std::env::var("ANTHROPIC_MODEL").ok(),
            // ...
        }
    }
}
```

**Note**: `EnvOnlyConfig` handles Anthropic routing (from TS). For multi-provider API keys, see `ProviderConfig.env_key` below — each provider resolves its own env var.

### Per-provider API key resolution (multi-provider, from cocode-rs)

```rust
/// TS only supports ANTHROPIC_API_KEY. Rust extends for multi-provider
/// using cocode-rs ProviderConfig pattern.
///
/// Each provider has an env_key for its API key.
/// Resolution: env var > config file api_key > error with hint.
pub struct ProviderConfig {
    pub name: String,
    pub api: ProviderApi,
    pub env_key: String,              // e.g. "OPENAI_API_KEY", "GOOGLE_API_KEY"
    pub api_key: Option<String>,      // fallback from config file
    pub base_url: String,
    pub default_model: Option<String>,
}

/// Built-in providers with their env_key.
pub fn builtin_providers() -> Vec<ProviderConfig> {
    vec![
        ProviderConfig {
            name: "anthropic".into(),
            api: ProviderApi::Anthropic,
            env_key: "ANTHROPIC_API_KEY".into(),
            base_url: "https://api.anthropic.com".into(),
            ..Default::default()
        },
        ProviderConfig {
            name: "openai".into(),
            api: ProviderApi::Openai,
            env_key: "OPENAI_API_KEY".into(),
            base_url: "https://api.openai.com/v1".into(),
            ..Default::default()
        },
        ProviderConfig {
            name: "google".into(),
            api: ProviderApi::Gemini,
            env_key: "GOOGLE_API_KEY".into(),
            base_url: "https://generativelanguage.googleapis.com".into(),
            ..Default::default()
        },
        // Volcengine, Z.AI, OpenAI-compatible...
    ]
}

/// Resolve API key for a provider.
/// 1. Check env var (env_key)
/// 2. Fall back to config file api_key
/// 3. Error with hint: "set {env_key} or add api_key to config"
pub fn resolve_api_key(provider: &ProviderConfig) -> Result<String, ConfigError>;
```
```

### Dual-source settings (both config file AND env var)

```rust
/// For settings that exist in BOTH config file and env var,
/// resolution order is explicitly documented per-setting.
///
/// Model: /model cmd > --model flag > ANTHROPIC_MODEL env > settings.model > default
/// API key: ANTHROPIC_API_KEY env > apiKeyHelper script > OAuth token
/// Effort: CLAUDE_CODE_EFFORT_LEVEL env > settings.effort_level > model default
```

### Env helper utilities (from `envUtils.ts`)

```rust
/// TS: isEnvTruthy() — normalizes "1", "true", "yes", "on" to true
pub fn is_env_truthy(key: &str) -> bool;

/// TS: isEnvDefinedFalsy() — normalizes "0", "false", "no", "off" to false
pub fn is_env_falsy(key: &str) -> bool;
```

---

## 4. Runtime Overrides (session state, not persisted)

```rust
/// Mutable state that changes during a session.
/// NOT persisted to config files. Lost when session ends.
/// TS: bootstrap/state.ts mainLoopModelOverride, etc.
pub struct RuntimeOverrides {
    pub model_override: Option<String>,         // /model command
    pub thinking_level_override: Option<ThinkingLevel>,  // /effort or /think command
    pub fast_mode_override: Option<bool>,       // /fast command
    pub permission_mode_override: Option<PermissionMode>,  // plan mode toggle
}

impl RuntimeOverrides {
    pub fn new() -> Self { Self::default() }
}
```

---

## 5. Config Resolution (combining all layers)

```rust
/// The fully resolved configuration for a session.
/// Combines: persisted Settings + EnvOnlyConfig + RuntimeOverrides.
pub struct ResolvedConfig {
    pub settings: Settings,                // merged from disk (immutable snapshot)
    pub env: EnvOnlyConfig,                // from environment
    pub overrides: RuntimeOverrides,       // mutable session state
    pub source_tracking: SettingsWithSource,
}

impl ResolvedConfig {
    /// Get the effective model for a role.
    /// Priority: override > env > settings > default
    pub fn model_for_role(&self, role: ModelRole) -> ModelSpec { ... }

    /// Get the effective API provider.
    /// TS: getAPIProvider()
    pub fn api_provider(&self) -> ProviderApi {
        if self.env.use_bedrock { return ProviderApi::Anthropic; } // Bedrock variant
        if self.env.use_vertex { return ProviderApi::Anthropic; }  // Vertex variant
        if self.env.use_foundry { return ProviderApi::Anthropic; } // Foundry variant
        // ... or from config for OpenAI/Google/etc.
        ProviderApi::Anthropic // default
    }

    /// Get the effective effort level.
    /// Priority: override > env > settings > model default
    pub fn effective_thinking_level(&self, model: &str) -> Option<ThinkingLevel> { ... }
}
```

---

## 6. Model Configuration

### ModelInfo (per-model, from config + defaults)

```rust
/// Rich per-model configuration. All fields optional for layered merging.
/// Aligned with cocode-rs common/protocol/src/model/model_info.rs.
///
/// Config layers (later overrides earlier):
///   builtin defaults → models.json → provider.models[model_id]
/// Merged via ModelInfo::merge_from() (other.Some overrides self).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    // === Identity ===
    pub model_id: String,
    pub display_name: Option<String>,

    // === Capacity ===
    pub context_window: i64,           // NOT Option — every model has one (default 200_000)
    pub max_output_tokens: i64,        // NOT Option — every model has one
    pub timeout_secs: Option<i64>,

    // === Capabilities ===
    pub capabilities: Option<Vec<Capability>>,

    // === Sampling ===
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<i64>,

    // === Thinking/Reasoning ===
    /// Supported thinking levels — the authority source for ThinkingLevel definitions.
    /// Each entry is a complete ThinkingLevel with effort + budget_tokens + options.
    /// User selects effort name → system resolves full entry from this list.
    pub supported_thinking_levels: Option<Vec<ThinkingLevel>>,
    /// Default effort level — a ref (ReasoningEffort name) into supported_thinking_levels.
    /// NOT a full ThinkingLevel — resolved at runtime by looking up the effort in the list.
    pub default_thinking_level: Option<ReasoningEffort>,

    // === Context Management ===
    pub auto_compact_pct: Option<i32>,

    // === Tools ===
    pub apply_patch_tool_type: Option<ApplyPatchToolType>,
    pub excluded_tools: Option<Vec<String>>,
    pub shell_type: Option<ConfigShellToolType>,
    pub max_tool_output_chars: Option<i32>,

    // === Instructions ===
    pub base_instructions: Option<String>,
    pub base_instructions_file: Option<String>,

    // === Provider-Specific Extensions ===
    /// Per-model provider options — merged into ProviderOptions at RequestBuilder Step 4.
    /// For non-thinking provider-specific params (e.g., store: false).
    /// Thinking-related params belong in ThinkingLevel.options instead.
    pub options: Option<HashMap<String, serde_json::Value>>,
}

impl ModelInfo {
    pub fn has_capability(&self, cap: Capability) -> bool {
        self.capabilities.as_ref().is_some_and(|caps| caps.contains(&cap))
    }

    /// Get default ThinkingLevel by looking up default effort in supported levels.
    pub fn default_thinking(&self) -> Option<&ThinkingLevel> {
        let effort = self.default_thinking_level?;
        self.supported_thinking_levels.as_ref()?
            .iter()
            .find(|l| l.effort == effort)
    }

    /// Resolve a requested effort to the best matching supported ThinkingLevel.
    pub fn resolve_thinking_level(&self, requested: &ThinkingLevel) -> ThinkingLevel {
        match &self.supported_thinking_levels {
            Some(levels) if !levels.is_empty() => {
                levels.iter()
                    .find(|l| l.effort == requested.effort)
                    .cloned()
                    .unwrap_or_else(|| {
                        // nearest match by effort distance
                        levels.iter()
                            .min_by_key(|l| (l.effort as i32 - requested.effort as i32).abs())
                            .cloned()
                            .unwrap_or_else(|| requested.clone())
                    })
            }
            _ => requested.clone(),
        }
    }

    /// Merge another config into this one (other.Some overrides self).
    pub fn merge_from(&mut self, other: &Self);
}
```

### ProviderInfo (runtime provider config)

```rust
/// Resolved provider configuration at runtime.
/// Aligned with cocode-rs common/protocol/src/provider.rs.
pub struct ProviderInfo {
    pub name: String,
    pub api: ProviderApi,
    pub base_url: String,
    pub api_key: String,
    pub timeout_secs: i64,                          // default: 600
    pub streaming: bool,                             // default: true
    pub wire_api: WireApi,
    /// Models registered under this provider (model_id → ProviderModel).
    pub models: HashMap<String, ProviderModel>,
    /// SDK client construction options (org_id, auth_token, headers, etc.).
    /// Used at provider init time, NOT per-request.
    pub options: Option<serde_json::Value>,
    /// HTTP interceptor chain names (applied as headers at RequestBuilder Step 5).
    pub interceptors: Vec<String>,
}

/// A model entry within a provider — combines ModelInfo with provider-specific overrides.
pub struct ProviderModel {
    /// Merged ModelInfo (builtin → models.json → provider config).
    #[serde(flatten)]
    pub model_info: ModelInfo,
    /// API model name if different from model_id (e.g., Bedrock endpoint ID).
    pub api_model_name: Option<String>,
    /// Per-provider per-model options — merged with ModelInfo.options in ModelHub,
    /// with these taking precedence.
    pub model_options: HashMap<String, serde_json::Value>,
}
```

### ModelRoles (role -> model mapping)

```rust
pub struct ModelRoles {
    pub roles: HashMap<ModelRole, ModelSpec>,
}

impl ModelRoles {
    pub fn get(&self, role: ModelRole) -> &ModelSpec {
        self.roles.get(&role).unwrap_or_else(|| &self.roles[&ModelRole::Main])
    }
}
```

### Model Selection Priority (from `utils/model/model.ts`)

```rust
/// Resolution order:
/// 1. RuntimeOverrides.model_override (/model command)
/// 2. CLI --model flag (stored in EnvOnlyConfig at startup)
/// 3. ANTHROPIC_MODEL env var (EnvOnlyConfig.model_override)
/// 4. Settings.model (merged config file field)
/// 5. Default by subscription tier / provider
pub fn get_main_loop_model(config: &ResolvedConfig) -> String;

/// Subagent: CLAUDE_CODE_SUBAGENT_MODEL env > tool model > agent model > "inherit"
pub fn get_agent_model(
    agent_model: Option<&str>,
    parent_spec: &ModelSpec,
    tool_model: Option<&str>,
    config: &ResolvedConfig,
) -> ModelSpec;
```

### Model Aliases

```rust
pub enum ModelAlias {
    Sonnet, Opus, Haiku, Best, SonnetLargeCtx, OpusLargeCtx, OpusPlan,
}

pub fn resolve_alias(alias: ModelAlias, provider: ProviderApi) -> String;
pub fn parse_user_model(input: &str) -> Result<String, ConfigError>;
```

---

## 7. Settings Watching (reuses utils/file-watch)

```rust
/// Uses utils/file-watch::FileWatcherBuilder (from cocode-rs, REUSE).
/// TS: changeDetector.ts with 1000ms stability threshold, 500ms poll.
///
/// DO NOT use notify directly — utils/file-watch already provides
/// event coalescing, throttling, and debounce.
use coco_file_watch::{FileWatcher, FileWatcherBuilder};

pub struct SettingsWatcher {
    inner: FileWatcher,  // from utils/file-watch
}

impl SettingsWatcher {
    pub fn new(
        cwd: &Path,
        on_change: impl Fn(SettingSource) + Send + 'static,
    ) -> Result<Self, ConfigError> {
        let watcher = FileWatcherBuilder::new()
            .debounce_ms(1000)  // TS: 1000ms stability threshold
            .watch(user_settings_path())
            .watch(project_settings_path(cwd))
            .watch(local_settings_path(cwd))
            .watch(managed_settings_path())
            .on_change(move |path| {
                let source = path_to_source(&path);
                on_change(source);
            })
            .build()?;
        Ok(Self { inner: watcher })
    }
}
```

---

## 8. Effort & Fast Mode

```rust
pub fn model_supports_effort(model_info: &ModelInfo) -> bool {
    model_info.supports(Capability::Effort)
}
pub fn model_supports_max_effort(model_info: &ModelInfo) -> bool {
    // Opus 4.6 only
    model_info.model_id.contains("opus-4-6")
}
pub fn get_default_thinking_level(model_info: &ModelInfo) -> Option<ThinkingLevel>;

/// Fast mode: same model (Opus 4.6), faster output speed. NOT a model switch.
pub enum FastModeState {
    Active,
    Cooldown { reset_at: i64, reason: CooldownReason },
}
pub enum CooldownReason { RateLimit, Overloaded }

/// Org-level availability check (from utils/fastMode.ts 533 LOC).
/// Returns (available, reason) where reason explains why not available.
/// Unavailability reasons:
///   "free" — free account tier (fast mode requires paid subscription)
///   "preference" — organization disabled fast mode via policy
///   "extra_usage_disabled" — requires billing, not enabled
///   "network_error" — org check failed (behind proxy)
///   "unknown" — unexpected failure
pub fn get_fast_mode_unavailable_reason(config: &ResolvedConfig) -> Option<String>;
pub fn is_fast_mode_available(config: &ResolvedConfig) -> bool;
pub fn get_fast_mode_model() -> String;  // "opus" or "opus[1m]"

/// Cooldown trigger: 429 (rate limit) or 503 (overloaded) during fast mode.
/// Duration: From reset_at timestamp (typically minutes).
/// Reset: Automatic when now >= reset_at.
pub fn trigger_cooldown(reason: CooldownReason, reset_at: i64);

/// Org status check endpoint: {BASE_API_URL}/api/claude_code_penguin_mode
/// Throttle: 30s minimum between requests.
/// Cache: penguinModeOrgEnabled persisted in global config.
/// Prefetch: runs at startup (prefetchFastModeStatus).
///   Checks auth scope (user:profile), OAuth vs API key.
///   30s throttle interval, in-flight promise memoization.
pub async fn prefetch_fast_mode_status();

/// Overage rejection: called when 429 includes
/// anthropic-ratelimit-unified-overage-disabled-reason header.
/// Reasons: out_of_credits, org_level_disabled, member_level_disabled,
///          seat_tier_level_disabled, overage_not_provisioned, etc.
/// Permanently disables fast mode (except out_of_credits/org_level_disabled_until).
pub fn handle_fast_mode_overage_rejection(reason: Option<&str>);

/// Per-session opt-in: when settings.fast_mode_per_session_opt_in is true,
/// fast mode starts OFF each session (user must explicitly enable with /fast).
pub fn get_initial_fast_mode_setting(
    model: &str,
    per_session_opt_in: bool,
) -> bool;

/// 1m merge: getFastModeModel() returns "opus" or "opus[1m]"
/// depending on is_opus_1m_merge_enabled() (context window expansion).
pub fn get_fast_mode_state(
    model: &str,
    user_enabled: Option<bool>,
) -> FastModeDisplayState;  // Off, Cooldown, On

pub enum FastModeDisplayState { Off, Cooldown, On }
```

---

## 9. Security Rules (from TS)

```rust
/// Security constraints on what project settings can do.
/// TS: project settings are restricted for security.
///
/// Project settings (.claude/settings.json) CANNOT set:
/// - apiKeyHelper (prevents RCE via path traversal)
/// - autoMemoryDirectory
/// - bypass permissions mode
/// - auto mode config
///
/// Only Policy settings can enforce:
/// - strictPluginOnlyCustomization (locks skills/hooks/agents to plugin-only)
/// - allowManagedPermissionRulesOnly
/// - allowManagedMcpServersOnly
```
