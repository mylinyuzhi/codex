use std::collections::HashMap;
use std::env::VarError;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fmt;

use strum::IntoEnumIterator;

/// Known environment variables owned or interpreted by coco.
///
/// Keep dynamic provider keys as strings; this enum is for stable env keys
/// that are part of coco's runtime/config surface.
///
/// `strum::EnumIter` is derived so `EnvKey::iter()` always stays in sync
/// with the enum definition — no hand-maintained parallel array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum EnvKey {
    AnthropicApiKey,
    AnthropicAuthToken,
    AnthropicBaseUrl,
    AnthropicFoundryResource,
    AnthropicVertexProjectId,
    CocoAgentColor,
    CocoAgentId,
    CocoAgentName,
    CocoAntTrace,
    CocoBashAutoBackgroundOnTimeout,
    CocoBubblewrap,
    CocoConfigDir,
    CocoDisableAutoMemory,
    CocoDisableFastMode,
    CocoDisableShellSnapshot,
    CocoExperimentalAgentTeams,
    CocoFileReadIgnorePatterns,
    CocoFoundryResource,
    CocoGlobTimeoutSeconds,
    CocoLang,
    CocoMaxContextTokens,
    CocoMaxToolUseConcurrency,
    CocoMemoryPathOverride,
    CocoMcpToolTimeoutMs,
    CocoModel,
    CocoParentSessionId,
    CocoPlanModeRequired,
    CocoRemote,
    CocoRemoteMemoryDir,
    CocoSandboxAllowNetwork,
    CocoSandboxEnabled,
    CocoSandboxExcludedCommands,
    CocoSandboxMode,
    CocoSessionEndHooksTimeoutMs,
    CocoShell,
    /// Prefix string injected before every hook command. Consumed by
    /// `coco_hooks::execute_hook` for Command-type hooks; NOT wired
    /// into `ShellConfig` / `ShellExecutor` (bash-tool uses its own
    /// settings.json path).
    CocoShellPrefix,
    CocoSimple,
    CocoSmallFastModel,
    CocoSubagentModel,
    CocoTaskListId,
    CocoTeamName,
    CocoTeammateCommand,
    CocoVerifyPlan,
}

impl EnvKey {
    /// Iterate over every known env key. Backed by `strum::EnumIter`, so
    /// adding a variant automatically shows up here.
    pub fn all() -> impl Iterator<Item = Self> {
        Self::iter()
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AnthropicApiKey => "ANTHROPIC_API_KEY",
            Self::AnthropicAuthToken => "ANTHROPIC_AUTH_TOKEN",
            Self::AnthropicBaseUrl => "ANTHROPIC_BASE_URL",
            Self::AnthropicFoundryResource => "ANTHROPIC_FOUNDRY_RESOURCE",
            Self::AnthropicVertexProjectId => "ANTHROPIC_VERTEX_PROJECT_ID",
            Self::CocoAgentColor => "COCO_AGENT_COLOR",
            Self::CocoAgentId => "COCO_AGENT_ID",
            Self::CocoAgentName => "COCO_AGENT_NAME",
            Self::CocoAntTrace => "COCO_ANT_TRACE",
            Self::CocoBashAutoBackgroundOnTimeout => "COCO_BASH_AUTO_BACKGROUND_ON_TIMEOUT",
            Self::CocoBubblewrap => "COCO_BUBBLEWRAP",
            Self::CocoConfigDir => "COCO_CONFIG_DIR",
            Self::CocoDisableAutoMemory => "COCO_DISABLE_AUTO_MEMORY",
            Self::CocoDisableFastMode => "COCO_DISABLE_FAST_MODE",
            Self::CocoDisableShellSnapshot => "COCO_DISABLE_SHELL_SNAPSHOT",
            Self::CocoExperimentalAgentTeams => "COCO_EXPERIMENTAL_AGENT_TEAMS",
            Self::CocoFileReadIgnorePatterns => "COCO_FILE_READ_IGNORE_PATTERNS",
            Self::CocoFoundryResource => "COCO_FOUNDRY_RESOURCE",
            Self::CocoGlobTimeoutSeconds => "COCO_GLOB_TIMEOUT_SECONDS",
            Self::CocoLang => "COCO_LANG",
            Self::CocoMaxContextTokens => "COCO_MAX_CONTEXT_TOKENS",
            Self::CocoMaxToolUseConcurrency => "COCO_MAX_TOOL_USE_CONCURRENCY",
            Self::CocoMemoryPathOverride => "COCO_MEMORY_PATH_OVERRIDE",
            Self::CocoMcpToolTimeoutMs => "COCO_MCP_TOOL_TIMEOUT_MS",
            Self::CocoModel => "COCO_MODEL",
            Self::CocoParentSessionId => "COCO_PARENT_SESSION_ID",
            Self::CocoPlanModeRequired => "COCO_PLAN_MODE_REQUIRED",
            Self::CocoRemote => "COCO_REMOTE",
            Self::CocoRemoteMemoryDir => "COCO_REMOTE_MEMORY_DIR",
            Self::CocoSandboxAllowNetwork => "COCO_SANDBOX_ALLOW_NETWORK",
            Self::CocoSandboxEnabled => "COCO_SANDBOX_ENABLED",
            Self::CocoSandboxExcludedCommands => "COCO_SANDBOX_EXCLUDED_COMMANDS",
            Self::CocoSandboxMode => "COCO_SANDBOX_MODE",
            Self::CocoSessionEndHooksTimeoutMs => "COCO_SESSIONEND_HOOKS_TIMEOUT_MS",
            Self::CocoShell => "COCO_SHELL",
            Self::CocoShellPrefix => "COCO_SHELL_PREFIX",
            Self::CocoSimple => "COCO_SIMPLE",
            Self::CocoSmallFastModel => "COCO_SMALL_FAST_MODEL",
            Self::CocoSubagentModel => "COCO_SUBAGENT_MODEL",
            Self::CocoTaskListId => "COCO_TASK_LIST_ID",
            Self::CocoTeamName => "COCO_TEAM_NAME",
            Self::CocoTeammateCommand => "COCO_TEAMMATE_COMMAND",
            Self::CocoVerifyPlan => "COCO_VERIFY_PLAN",
        }
    }
}

impl AsRef<OsStr> for EnvKey {
    fn as_ref(&self) -> &OsStr {
        OsStr::new(self.as_str())
    }
}

impl fmt::Display for EnvKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Wrapper around `std::env::var` that accepts `EnvKey` directly.
pub fn var<K: AsRef<OsStr>>(key: K) -> Result<String, VarError> {
    std::env::var(key)
}

/// Wrapper around `std::env::var_os` that accepts `EnvKey` directly.
pub fn var_os<K: AsRef<OsStr>>(key: K) -> Option<OsString> {
    std::env::var_os(key)
}

/// Normalize a raw env value against the truthy set ("1"/"true"/"yes"/"on").
fn is_truthy_value(raw: &str) -> bool {
    matches!(raw.to_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

/// Normalize a raw env value against the falsy set ("0"/"false"/"no"/"off").
fn is_falsy_value(raw: &str) -> bool {
    matches!(raw.to_lowercase().as_str(), "0" | "false" | "no" | "off")
}

/// Returns true if the environment variable is set to a truthy value.
/// TS: isEnvTruthy() — normalizes "1", "true", "yes", "on" to true.
pub fn is_env_truthy<K: AsRef<OsStr>>(key: K) -> bool {
    var(key).ok().is_some_and(|v| is_truthy_value(&v))
}

/// Returns true if the environment variable is set to a falsy value.
/// TS: isEnvDefinedFalsy() — normalizes "0", "false", "no", "off".
pub fn is_env_falsy<K: AsRef<OsStr>>(key: K) -> bool {
    var(key).ok().is_some_and(|v| is_falsy_value(&v))
}

/// Get an environment variable as an optional string.
pub fn env_opt<K: AsRef<OsStr>>(key: K) -> Option<String> {
    var(key).ok().filter(|v| !v.is_empty())
}

/// Get an environment variable as an optional i32.
pub fn env_opt_i32<K: AsRef<OsStr>>(key: K) -> Option<i32> {
    env_opt(key).and_then(|v| v.parse().ok())
}

/// Get an environment variable as an optional i64.
pub fn env_opt_i64<K: AsRef<OsStr>>(key: K) -> Option<i64> {
    env_opt(key).and_then(|v| v.parse().ok())
}

/// Startup snapshot of stable coco-owned environment variables.
#[derive(Debug, Clone, Default)]
pub struct EnvSnapshot {
    values: HashMap<EnvKey, String>,
}

impl EnvSnapshot {
    /// Capture known env vars from the current process.
    pub fn from_current_process() -> Self {
        let values = EnvKey::all()
            .filter_map(|key| env_opt(key).map(|value| (key, value)))
            .collect();
        Self { values }
    }

    /// Build a snapshot from explicit pairs. Intended for tests and callers
    /// that already captured their environment.
    pub fn from_pairs<I, S>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (EnvKey, S)>,
        S: Into<String>,
    {
        Self {
            values: pairs
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
        }
    }

    pub fn get(&self, key: EnvKey) -> Option<&str> {
        self.values.get(&key).map(String::as_str)
    }

    pub fn get_string(&self, key: EnvKey) -> Option<String> {
        self.get(key).map(str::to_string)
    }

    pub fn get_i32(&self, key: EnvKey) -> Option<i32> {
        self.get(key).and_then(|value| value.parse().ok())
    }

    pub fn get_i64(&self, key: EnvKey) -> Option<i64> {
        self.get(key).and_then(|value| value.parse().ok())
    }

    pub fn is_truthy(&self, key: EnvKey) -> bool {
        self.get(key).is_some_and(is_truthy_value)
    }

    pub fn is_falsy(&self, key: EnvKey) -> bool {
        self.get(key).is_some_and(is_falsy_value)
    }
}

/// Env-only config. No Settings file equivalent.
///
/// Only holds env vars that have **no** corresponding typed section on
/// `RuntimeConfig`. Anything that also flows into a section (tool, shell,
/// memory, sandbox, mcp, …) is intentionally omitted to avoid two
/// consumers resolving the same knob to different values.
///
/// Bedrock / Vertex / Foundry routing env vars were removed — those
/// providers aren't shipped in coco-rs today. Re-add alongside the
/// provider crate when they land.
#[derive(Debug, Clone, Default)]
pub struct EnvOnlyConfig {
    // Model overrides (consumed by `resolve_model_roles`; not duplicated
    // into a typed section because `ModelRoles` already is the section).
    pub model_override: Option<String>,
    pub small_fast_model: Option<String>,
    pub subagent_model: Option<String>,

    /// `COCO_SIMPLE` — skips stored auth tokens / api_key_helper fallback
    /// and forces env-only API key resolution. Consumed by
    /// `coco_inference::auth::resolve_auth` via `AuthResolveOptions`.
    pub bare_mode: bool,
}

impl EnvOnlyConfig {
    /// Read all env vars once at startup.
    pub fn from_env() -> Self {
        Self::from_snapshot(&EnvSnapshot::from_current_process())
    }

    pub fn from_snapshot(env: &EnvSnapshot) -> Self {
        Self {
            model_override: env.get_string(EnvKey::CocoModel),
            small_fast_model: env.get_string(EnvKey::CocoSmallFastModel),
            subagent_model: env.get_string(EnvKey::CocoSubagentModel),
            bare_mode: env.is_truthy(EnvKey::CocoSimple),
        }
    }
}

#[cfg(test)]
#[path = "env.test.rs"]
mod tests;
