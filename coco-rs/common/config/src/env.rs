/// Returns true if the environment variable is set to a truthy value.
/// TS: isEnvTruthy() — normalizes "1", "true", "yes", "on" to true.
pub fn is_env_truthy(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .is_some_and(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
}

/// Returns true if the environment variable is set to a falsy value.
/// TS: isEnvDefinedFalsy() — normalizes "0", "false", "no", "off".
pub fn is_env_falsy(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .is_some_and(|v| matches!(v.to_lowercase().as_str(), "0" | "false" | "no" | "off"))
}

/// Get an environment variable as an optional string.
pub fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Get an environment variable as an optional i32.
pub fn env_opt_i32(key: &str) -> Option<i32> {
    env_opt(key).and_then(|v| v.parse().ok())
}

/// Get an environment variable as an optional i64.
pub fn env_opt_i64(key: &str) -> Option<i64> {
    env_opt(key).and_then(|v| v.parse().ok())
}

/// Env-only config. No Settings file equivalent.
/// Read once at startup.
#[derive(Debug, Clone, Default)]
pub struct EnvOnlyConfig {
    // Anthropic deployment routing (from TS)
    pub use_bedrock: bool,
    pub use_vertex: bool,
    pub use_foundry: bool,

    // Model override (higher priority than settings.model)
    pub model_override: Option<String>,
    pub small_fast_model: Option<String>,
    pub subagent_model: Option<String>,

    // Shell
    pub shell_override: Option<String>,

    // Limits
    pub max_tool_concurrency: Option<i32>,
    pub max_context_tokens: Option<i64>,

    // Runtime flags
    pub bare_mode: bool,
}

impl EnvOnlyConfig {
    /// Read all env vars once at startup.
    pub fn from_env() -> Self {
        Self {
            use_bedrock: is_env_truthy("CLAUDE_CODE_USE_BEDROCK"),
            use_vertex: is_env_truthy("CLAUDE_CODE_USE_VERTEX"),
            use_foundry: is_env_truthy("CLAUDE_CODE_USE_FOUNDRY"),
            model_override: env_opt("ANTHROPIC_MODEL").or_else(|| env_opt("COCO_MODEL")),
            small_fast_model: env_opt("ANTHROPIC_SMALL_FAST_MODEL"),
            subagent_model: env_opt("CLAUDE_CODE_SUBAGENT_MODEL"),
            shell_override: env_opt("CLAUDE_CODE_SHELL"),
            max_tool_concurrency: env_opt_i32("CLAUDE_CODE_MAX_TOOL_USE_CONCURRENCY"),
            max_context_tokens: env_opt_i64("CLAUDE_CODE_MAX_CONTEXT_TOKENS"),
            bare_mode: is_env_truthy("CLAUDE_CODE_SIMPLE"),
        }
    }
}

#[cfg(test)]
#[path = "env.test.rs"]
mod tests;
