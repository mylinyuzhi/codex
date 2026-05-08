use std::path::PathBuf;

use coco_types::PermissionMode;
use serde::Deserialize;
use serde::Serialize;

use crate::env::EnvKey;
use crate::env::EnvSnapshot;
use crate::settings::Settings;

const DEFAULT_MAX_TOOL_CONCURRENCY: i32 = 10;
const DEFAULT_MAX_RESULT_SIZE: i32 = 400_000;
const DEFAULT_RESULT_PREVIEW_SIZE: i32 = 2_000;
const DEFAULT_BASH_TIMEOUT_MS: i64 = 120_000;
const DEFAULT_BASH_MAX_TIMEOUT_MS: i64 = 600_000;
const DEFAULT_BASH_MAX_OUTPUT_BYTES: i64 = 30_000;
/// Upper cap on Bash output length — larger configured values are clamped
/// down at `finalize()` time. TS: `utils/shell/outputLimits.ts` —
/// `BASH_MAX_OUTPUT_UPPER_LIMIT = 150_000`.
///
/// Crate-internal: this is a config-resolution detail, not a public API.
pub(crate) const BASH_MAX_OUTPUT_BYTES_UPPER: i64 = 150_000;
const DEFAULT_GLOB_TIMEOUT_SECONDS: i32 = 10;
const DEFAULT_MAX_RETRIES: i32 = 3;
const DEFAULT_RETRY_BASE_DELAY_MS: i64 = 1_000;
const DEFAULT_RETRY_MAX_DELAY_MS: i64 = 60_000;
const DEFAULT_RETRY_JITTER: f64 = 0.25;
/// 60-second HTTP fetch timeout — matches TS `WebFetchTool/utils.ts:116`
/// `FETCH_TIMEOUT_MS = 60_000`. Long enough for slow origins, short
/// enough that the model doesn't stall forever on a stuck fetch.
const DEFAULT_WEB_FETCH_TIMEOUT_SECS: i64 = 60;
/// 100K-char extraction budget. Matches TS `utils.ts:128`
/// `MAX_MARKDOWN_LENGTH = 100_000`. Guards side-query token cost.
const DEFAULT_WEB_FETCH_MAX_CONTENT_LENGTH: i64 = 100_000;
/// Default user agent — mirrors TS `Claude-User (...)` so robots.txt
/// rules targeting Claude-Code's fetcher apply identically to coco-rs.
const DEFAULT_WEB_FETCH_USER_AGENT: &str =
    "Claude-User (claude-code/coco-rs; +https://support.anthropic.com/)";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PartialToolSettings {
    pub max_tool_concurrency: Option<i32>,
    pub max_result_size: Option<i32>,
    pub result_preview_size: Option<i32>,
    pub enable_result_persistence: Option<bool>,
    pub glob_timeout_seconds: Option<i32>,
    pub file_read_ignore_patterns: Option<Vec<String>>,
    pub bash: Option<PartialBashSettings>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PartialBashSettings {
    pub default_timeout_ms: Option<i64>,
    pub max_timeout_ms: Option<i64>,
    pub max_output_bytes: Option<i64>,
    pub auto_background_on_timeout: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolConfig {
    pub max_tool_concurrency: i32,
    pub max_result_size: i32,
    pub result_preview_size: i32,
    pub enable_result_persistence: bool,
    pub glob_timeout_seconds: i32,
    pub file_read_ignore_patterns: Vec<String>,
    pub bash: BashConfig,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            max_tool_concurrency: DEFAULT_MAX_TOOL_CONCURRENCY,
            max_result_size: DEFAULT_MAX_RESULT_SIZE,
            result_preview_size: DEFAULT_RESULT_PREVIEW_SIZE,
            enable_result_persistence: true,
            glob_timeout_seconds: DEFAULT_GLOB_TIMEOUT_SECONDS,
            file_read_ignore_patterns: Vec::new(),
            bash: BashConfig::default(),
        }
    }
}

impl ToolConfig {
    pub fn resolve(settings: &Settings, env: &EnvSnapshot) -> Self {
        let mut config = Self::default();
        let tool = &settings.tool;

        if let Some(v) = tool.max_tool_concurrency {
            config.max_tool_concurrency = v;
        }
        if let Some(v) = tool.max_result_size {
            config.max_result_size = v;
        }
        if let Some(v) = tool.result_preview_size {
            config.result_preview_size = v;
        }
        if let Some(v) = tool.enable_result_persistence {
            config.enable_result_persistence = v;
        }
        if let Some(v) = tool.glob_timeout_seconds {
            config.glob_timeout_seconds = v;
        }
        if let Some(patterns) = &tool.file_read_ignore_patterns {
            config.file_read_ignore_patterns.clone_from(patterns);
        }
        if let Some(bash) = &tool.bash {
            config.bash.apply_settings(bash);
        }

        if let Some(v) = env.get_i32(EnvKey::CocoMaxToolUseConcurrency) {
            config.max_tool_concurrency = v;
        }
        if let Some(v) = env.get_i32(EnvKey::CocoGlobTimeoutSeconds) {
            config.glob_timeout_seconds = v;
        }
        if let Some(raw) = env.get(EnvKey::CocoFileReadIgnorePatterns) {
            config.file_read_ignore_patterns = raw
                .split([':', ','])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
        }
        if env.is_truthy(EnvKey::CocoBashAutoBackgroundOnTimeout) {
            config.bash.auto_background_on_timeout = true;
        } else if env.is_falsy(EnvKey::CocoBashAutoBackgroundOnTimeout) {
            config.bash.auto_background_on_timeout = false;
        }

        config.finalize();
        config
    }

    fn finalize(&mut self) {
        self.max_tool_concurrency = self.max_tool_concurrency.max(1);
        self.max_result_size = self.max_result_size.max(0);
        self.result_preview_size = self.result_preview_size.max(0);
        self.glob_timeout_seconds = self.glob_timeout_seconds.max(1);
        self.bash.finalize();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BashConfig {
    pub default_timeout_ms: i64,
    pub max_timeout_ms: i64,
    pub max_output_bytes: i64,
    pub auto_background_on_timeout: bool,
}

impl Default for BashConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: DEFAULT_BASH_TIMEOUT_MS,
            max_timeout_ms: DEFAULT_BASH_MAX_TIMEOUT_MS,
            max_output_bytes: DEFAULT_BASH_MAX_OUTPUT_BYTES,
            auto_background_on_timeout: false,
        }
    }
}

impl BashConfig {
    fn apply_settings(&mut self, settings: &PartialBashSettings) {
        if let Some(v) = settings.default_timeout_ms {
            self.default_timeout_ms = v;
        }
        if let Some(v) = settings.max_timeout_ms {
            self.max_timeout_ms = v;
        }
        if let Some(v) = settings.max_output_bytes {
            self.max_output_bytes = v;
        }
        if let Some(v) = settings.auto_background_on_timeout {
            self.auto_background_on_timeout = v;
        }
    }

    fn finalize(&mut self) {
        self.default_timeout_ms = self.default_timeout_ms.max(1);
        self.max_timeout_ms = self.max_timeout_ms.max(self.default_timeout_ms);
        self.max_output_bytes = self.max_output_bytes.clamp(0, BASH_MAX_OUTPUT_BYTES_UPPER);
    }
}

// Compaction settings live in `crate::compact_settings`
// (`CompactConfig` and its sub-structs). Per-invocation run-options for
// `compact_conversation` live in `coco_compact::CompactRunOptions`.
// The two are intentionally distinct types: the former is the global
// resolved-from-settings struct; the latter is the per-call parameter
// bag.

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PartialApiSettings {
    pub retry: Option<PartialApiRetrySettings>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PartialApiRetrySettings {
    pub max_retries: Option<i32>,
    pub base_delay_ms: Option<i64>,
    pub max_delay_ms: Option<i64>,
    pub jitter_factor: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ApiConfig {
    pub retry: ApiRetryConfig,
}

impl ApiConfig {
    pub fn resolve(settings: &Settings) -> Self {
        let mut config = Self::default();
        if let Some(retry) = &settings.api.retry {
            config.retry.apply_settings(retry);
        }
        config.retry.finalize();
        config
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiRetryConfig {
    pub max_retries: i32,
    pub base_delay_ms: i64,
    pub max_delay_ms: i64,
    pub jitter_factor: f64,
}

impl Default for ApiRetryConfig {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            base_delay_ms: DEFAULT_RETRY_BASE_DELAY_MS,
            max_delay_ms: DEFAULT_RETRY_MAX_DELAY_MS,
            jitter_factor: DEFAULT_RETRY_JITTER,
        }
    }
}

impl ApiRetryConfig {
    fn apply_settings(&mut self, settings: &PartialApiRetrySettings) {
        if let Some(v) = settings.max_retries {
            self.max_retries = v;
        }
        if let Some(v) = settings.base_delay_ms {
            self.base_delay_ms = v;
        }
        if let Some(v) = settings.max_delay_ms {
            self.max_delay_ms = v;
        }
        if let Some(v) = settings.jitter_factor {
            self.jitter_factor = v;
        }
    }

    fn finalize(&mut self) {
        self.max_retries = self.max_retries.max(0);
        self.base_delay_ms = self.base_delay_ms.max(0);
        self.max_delay_ms = self.max_delay_ms.max(self.base_delay_ms);
        self.jitter_factor = self.jitter_factor.clamp(0.0, 1.0);
    }
}

// `ApiFallbackConfig` previously lived here. Removed — no consumer.
// Stream-fallback, overflow-recovery, and the escalated-max-tokens
// value all live inside `app/query::engine` today as named constants
// (`ESCALATED_MAX_TOKENS`, `MAX_OUTPUT_TOKENS_RECOVERY_LIMIT`).

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PartialLoopSettings {
    pub max_turns: Option<i32>,
    pub max_tokens: Option<i32>,
    pub permission_mode: Option<PermissionMode>,
    pub enable_streaming_tools: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopConfig {
    pub max_turns: Option<i32>,
    pub max_tokens: Option<i32>,
    pub permission_mode: PermissionMode,
    pub enable_streaming_tools: bool,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_turns: Some(30),
            max_tokens: None,
            permission_mode: PermissionMode::Default,
            enable_streaming_tools: true,
        }
    }
}

impl LoopConfig {
    pub fn resolve(settings: &Settings, overrides: &crate::RuntimeOverrides) -> Self {
        let mut config = Self::default();
        let loop_settings = &settings.loop_config;

        if loop_settings.max_turns.is_some() {
            config.max_turns = loop_settings.max_turns;
        }
        if loop_settings.max_tokens.is_some() {
            config.max_tokens = loop_settings.max_tokens;
        }
        if let Some(mode) = loop_settings.permission_mode {
            config.permission_mode = mode;
        }
        if let Some(v) = loop_settings.enable_streaming_tools {
            config.enable_streaming_tools = v;
        }
        if let Some(mode) = overrides.permission_mode_override {
            config.permission_mode = mode;
        }
        config
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PartialShellSettings {
    pub default_shell: Option<String>,
    pub disable_snapshot: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellConfig {
    pub default_shell: Option<String>,
    pub disable_snapshot: bool,
}

impl ShellConfig {
    pub fn resolve(settings: &Settings, env: &EnvSnapshot) -> Self {
        let mut config = Self {
            default_shell: settings.shell.default_shell.clone(),
            disable_snapshot: settings.shell.disable_snapshot.unwrap_or(false),
        };
        if let Some(shell) = env.get_string(EnvKey::CocoShell) {
            config.default_shell = Some(shell);
        }
        if env.is_truthy(EnvKey::CocoDisableShellSnapshot) {
            config.disable_snapshot = true;
        }
        config
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PartialMemorySettings {
    pub directory: Option<PathBuf>,
    pub skip_index: Option<bool>,
    pub kairos_mode: Option<bool>,

    // Extraction (turn-end forked agent — services/extractMemories)
    pub extraction_enabled: Option<bool>,
    pub extraction_throttle: Option<i32>,
    pub extraction_max_turns: Option<i32>,

    // Team memory
    pub team_memory_enabled: Option<bool>,

    // Auto-dream consolidation (services/autoDream)
    pub dream_enabled: Option<bool>,
    pub dream_min_hours: Option<i32>,
    pub dream_min_sessions: Option<i32>,

    // Session memory (services/SessionMemory) — distinct from compact's
    pub session_memory_enabled: Option<bool>,
    pub session_memory_init_tokens: Option<i64>,
    pub session_memory_update_tokens: Option<i64>,
    pub session_memory_tool_calls: Option<i32>,
    pub session_memory_per_section_tokens: Option<i64>,
    pub session_memory_total_tokens: Option<i64>,
}

/// Resolved auto-memory configuration.
///
/// Whether the subsystem is **active** is gated upstream by
/// `Feature::AutoMemory`; this struct only carries internal sub-toggles
/// and parameters. Sub-toggles for extraction, team memory, auto-dream,
/// and session memory all live here as flat fields with prefix naming
/// — there is no separate `*Config` per subsystem (matches the project
/// convention: one `Feature` gate, all sub-toggles flat in the owning
/// `*Config`).
///
/// Source of truth for `coco_memory::MemoryConfig` (thin adapter).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub directory: Option<PathBuf>,
    pub skip_index: bool,
    pub kairos_mode: bool,

    /// Extraction (turn-end forked agent).
    pub extraction_enabled: bool,
    pub extraction_throttle: i32,
    pub extraction_max_turns: i32,

    /// Team memory (memdir/team subdir).
    pub team_memory_enabled: bool,

    /// Auto-dream consolidation.
    pub dream_enabled: bool,
    pub dream_min_hours: i32,
    pub dream_min_sessions: i32,

    /// Session memory — TS-aligned defaults, distinct feature from
    /// `compact_settings::SessionMemoryConfig`.
    pub session_memory_enabled: bool,
    pub session_memory_init_tokens: i64,
    pub session_memory_update_tokens: i64,
    pub session_memory_tool_calls: i32,
    pub session_memory_per_section_tokens: i64,
    pub session_memory_total_tokens: i64,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            directory: None,
            skip_index: false,
            kairos_mode: false,
            extraction_enabled: true,
            extraction_throttle: 1,
            extraction_max_turns: 5,
            team_memory_enabled: false,
            dream_enabled: true,
            dream_min_hours: 24,
            dream_min_sessions: 5,
            session_memory_enabled: true,
            session_memory_init_tokens: 10_000,
            session_memory_update_tokens: 5_000,
            session_memory_tool_calls: 3,
            session_memory_per_section_tokens: 2_000,
            session_memory_total_tokens: 12_000,
        }
    }
}

impl MemoryConfig {
    pub fn resolve(settings: &Settings, env: &EnvSnapshot) -> Self {
        let mut config = Self::default();
        let s = &settings.memory;

        if let Some(dir) = &s.directory {
            config.directory = Some(dir.clone());
        }
        if let Some(v) = s.skip_index {
            config.skip_index = v;
        }
        if let Some(v) = s.kairos_mode {
            config.kairos_mode = v;
        }
        if let Some(v) = s.extraction_enabled {
            config.extraction_enabled = v;
        }
        if let Some(v) = s.extraction_throttle {
            config.extraction_throttle = v;
        }
        if let Some(v) = s.extraction_max_turns {
            config.extraction_max_turns = v;
        }
        if let Some(v) = s.team_memory_enabled {
            config.team_memory_enabled = v;
        }
        if let Some(v) = s.dream_enabled {
            config.dream_enabled = v;
        }
        if let Some(v) = s.dream_min_hours {
            config.dream_min_hours = v;
        }
        if let Some(v) = s.dream_min_sessions {
            config.dream_min_sessions = v;
        }
        if let Some(v) = s.session_memory_enabled {
            config.session_memory_enabled = v;
        }
        if let Some(v) = s.session_memory_init_tokens {
            config.session_memory_init_tokens = v;
        }
        if let Some(v) = s.session_memory_update_tokens {
            config.session_memory_update_tokens = v;
        }
        if let Some(v) = s.session_memory_tool_calls {
            config.session_memory_tool_calls = v;
        }
        if let Some(v) = s.session_memory_per_section_tokens {
            config.session_memory_per_section_tokens = v;
        }
        if let Some(v) = s.session_memory_total_tokens {
            config.session_memory_total_tokens = v;
        }

        // Path override: `CocoMemoryPathOverride` is the operator-facing
        // local override; `CocoRemoteMemoryDir` is piped from the swarm
        // leader into teammates so in-process members share a memory root
        // without the operator having to re-export manually. Local
        // override wins if both are set.
        if let Some(dir) = env
            .get_string(EnvKey::CocoMemoryPathOverride)
            .or_else(|| env.get_string(EnvKey::CocoRemoteMemoryDir))
        {
            config.directory = Some(PathBuf::from(dir));
        }

        // Force-disable env overrides (truthy = disable). Settings can
        // already say "off"; these env vars only ever turn things off.
        if env.is_truthy(EnvKey::CocoMemoryExtractionDisable) {
            config.extraction_enabled = false;
        }
        if env.is_truthy(EnvKey::CocoMemoryDreamDisable) {
            config.dream_enabled = false;
        }
        if env.is_truthy(EnvKey::CocoMemorySessionMemoryDisable) {
            config.session_memory_enabled = false;
        }
        if env.is_truthy(EnvKey::CocoMemoryKairos) {
            config.kairos_mode = true;
        }

        // Clamps. Negative / zero values would break the gates.
        config.extraction_throttle = config.extraction_throttle.max(1);
        config.extraction_max_turns = config.extraction_max_turns.max(1);
        config.dream_min_hours = config.dream_min_hours.max(1);
        config.dream_min_sessions = config.dream_min_sessions.max(1);
        config.session_memory_init_tokens = config.session_memory_init_tokens.max(1);
        config.session_memory_update_tokens = config.session_memory_update_tokens.max(1);
        config.session_memory_tool_calls = config.session_memory_tool_calls.max(1);
        config.session_memory_per_section_tokens = config.session_memory_per_section_tokens.max(1);
        config.session_memory_total_tokens = config.session_memory_total_tokens.max(1);
        config
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PartialMcpRuntimeSettings {
    pub tool_timeout_ms: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpRuntimeConfig {
    pub tool_timeout_ms: Option<i32>,
}

impl McpRuntimeConfig {
    pub fn resolve(settings: &Settings, env: &EnvSnapshot) -> Self {
        let mut config = Self::default();
        if let Some(v) = settings.mcp_runtime.tool_timeout_ms {
            config.tool_timeout_ms = Some(v);
        }
        if let Some(v) = env.get_i32(EnvKey::CocoMcpToolTimeoutMs) {
            config.tool_timeout_ms = Some(v);
        }
        if let Some(v) = config.tool_timeout_ms {
            config.tool_timeout_ms = Some(v.max(1));
        }
        config
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PartialWebFetchSettings {
    pub timeout_secs: Option<i64>,
    pub max_content_length: Option<i64>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebFetchConfig {
    pub timeout_secs: i64,
    pub max_content_length: i64,
    pub user_agent: String,
}

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self {
            timeout_secs: DEFAULT_WEB_FETCH_TIMEOUT_SECS,
            max_content_length: DEFAULT_WEB_FETCH_MAX_CONTENT_LENGTH,
            user_agent: DEFAULT_WEB_FETCH_USER_AGENT.to_string(),
        }
    }
}

impl WebFetchConfig {
    pub fn resolve(settings: &Settings) -> Self {
        let mut config = Self::default();
        if let Some(v) = settings.web_fetch.timeout_secs {
            config.timeout_secs = v;
        }
        if let Some(v) = settings.web_fetch.max_content_length {
            config.max_content_length = v;
        }
        if let Some(v) = &settings.web_fetch.user_agent {
            config.user_agent.clone_from(v);
        }
        config.timeout_secs = config.timeout_secs.max(1);
        config.max_content_length = config.max_content_length.max(0);
        config
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PartialWebSearchSettings {
    pub provider: Option<WebSearchProvider>,
    pub max_results: Option<i32>,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchProvider {
    #[default]
    DuckDuckGo,
    Tavily,
    OpenAi,
}

impl WebSearchProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DuckDuckGo => "duckduckgo",
            Self::Tavily => "tavily",
            Self::OpenAi => "openai",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSearchConfig {
    pub provider: WebSearchProvider,
    pub max_results: i32,
    pub api_key: Option<String>,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            provider: WebSearchProvider::DuckDuckGo,
            max_results: 5,
            api_key: None,
        }
    }
}

impl WebSearchConfig {
    pub fn resolve(settings: &Settings) -> Self {
        let mut config = Self::default();
        if let Some(v) = settings.web_search.provider {
            config.provider = v;
        }
        if let Some(v) = settings.web_search.max_results {
            config.max_results = v;
        }
        if let Some(v) = &settings.web_search.api_key {
            config.api_key = Some(v.clone());
        }
        config.max_results = config.max_results.clamp(1, 20);
        config
    }
}

// `AttachmentConfig` previously lived here. Removed — no consumer,
// and the two fields (`disable_attachments`,
// `enable_token_usage_attachment`) weren't wired into
// `coco_context::attachment`. Re-add when the attachment pipeline
// grows explicit on/off gates.

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PartialPathSettings {
    pub project_dir: Option<PathBuf>,
}

/// Resolved filesystem paths. Only `project_dir` ships today — the
/// unused `plugin_root` / `env_file` slots were removed (consumers
/// elsewhere read them from their own scopes rather than this struct).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathConfig {
    pub project_dir: Option<PathBuf>,
}

impl PathConfig {
    pub fn resolve(settings: &Settings) -> Self {
        Self {
            project_dir: settings.paths.project_dir.clone(),
        }
    }
}

#[cfg(test)]
#[path = "sections.test.rs"]
mod tests;
