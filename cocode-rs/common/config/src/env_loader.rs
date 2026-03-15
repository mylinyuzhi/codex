//! Environment variable loading for configuration.
//!
//! Loads configuration from environment variables with support for both
//! `COCODE_` and `CLAUDE_CODE_` prefixes (for compatibility).

use cocode_protocol::AttachmentConfig;
use cocode_protocol::CompactConfig;
use cocode_protocol::PathConfig;
use cocode_protocol::PlanModeConfig;
use cocode_protocol::ToolConfig;
use std::env;
use std::path::PathBuf;
use tracing::debug;

// ============================================================
// Environment Variable Names
// ============================================================

// Tool execution
pub const ENV_MAX_TOOL_CONCURRENCY: &str = "COCODE_MAX_TOOL_USE_CONCURRENCY";
pub const ENV_MCP_TOOL_TIMEOUT: &str = "MCP_TOOL_TIMEOUT";

// Paths
pub const ENV_PROJECT_DIR: &str = "COCODE_PROJECT_DIR";
pub const ENV_PLUGIN_ROOT: &str = "COCODE_PLUGIN_ROOT";
pub const ENV_ENV_FILE: &str = "COCODE_ENV_FILE";

// Compaction - Feature toggles and overrides
pub const ENV_DISABLE_COMPACT: &str = "DISABLE_COMPACT";
pub const ENV_DISABLE_AUTO_COMPACT: &str = "DISABLE_AUTO_COMPACT";
pub const ENV_DISABLE_MICRO_COMPACT: &str = "DISABLE_MICRO_COMPACT";
pub const ENV_AUTOCOMPACT_PCT: &str = "COCODE_AUTOCOMPACT_PCT_OVERRIDE";
pub const ENV_BLOCKING_LIMIT: &str = "COCODE_BLOCKING_LIMIT_OVERRIDE";

// Compaction - Session memory
pub const ENV_SESSION_MEMORY_MIN: &str = "COCODE_SESSION_MEMORY_MIN_TOKENS";
pub const ENV_SESSION_MEMORY_MAX: &str = "COCODE_SESSION_MEMORY_MAX_TOKENS";
pub const ENV_EXTRACTION_COOLDOWN: &str = "COCODE_EXTRACTION_COOLDOWN_SECS";

// Compaction - Context restoration
pub const ENV_CONTEXT_RESTORE_FILES: &str = "COCODE_CONTEXT_RESTORE_MAX_FILES";
pub const ENV_CONTEXT_RESTORE_BUDGET: &str = "COCODE_CONTEXT_RESTORE_BUDGET";
pub const ENV_MAX_TOKENS_PER_FILE: &str = "COCODE_MAX_TOKENS_PER_FILE";

// Compaction - Threshold control
pub const ENV_MIN_TOKENS_TO_PRESERVE: &str = "COCODE_MIN_TOKENS_TO_PRESERVE";
pub const ENV_WARNING_THRESHOLD_OFFSET: &str = "COCODE_WARNING_THRESHOLD_OFFSET";
pub const ENV_ERROR_THRESHOLD_OFFSET: &str = "COCODE_ERROR_THRESHOLD_OFFSET";
pub const ENV_MIN_BLOCKING_OFFSET: &str = "COCODE_MIN_BLOCKING_OFFSET";

// Compaction - Micro-compact
pub const ENV_MICRO_COMPACT_MIN_SAVINGS: &str = "COCODE_MICRO_COMPACT_MIN_SAVINGS";
pub const ENV_MICRO_COMPACT_THRESHOLD: &str = "COCODE_MICRO_COMPACT_THRESHOLD";
pub const ENV_RECENT_TOOL_RESULTS_TO_KEEP: &str = "COCODE_RECENT_TOOL_RESULTS_TO_KEEP";

// Compaction - Full compact
pub const ENV_MAX_SUMMARY_RETRIES: &str = "COCODE_MAX_SUMMARY_RETRIES";
pub const ENV_MAX_COMPACT_OUTPUT_TOKENS: &str = "COCODE_MAX_COMPACT_OUTPUT_TOKENS";
pub const ENV_TOKEN_SAFETY_MARGIN: &str = "COCODE_TOKEN_SAFETY_MARGIN";
pub const ENV_TOKENS_PER_IMAGE: &str = "COCODE_TOKENS_PER_IMAGE";

// Plan mode
pub const ENV_PLAN_AGENT_COUNT: &str = "COCODE_PLAN_AGENT_COUNT";
pub const ENV_PLAN_EXPLORE_AGENT_COUNT: &str = "COCODE_PLAN_EXPLORE_AGENT_COUNT";

// Attachments
pub const ENV_DISABLE_ATTACHMENTS: &str = "COCODE_DISABLE_ATTACHMENTS";
pub const ENV_ENABLE_TOKEN_USAGE: &str = "COCODE_ENABLE_TOKEN_USAGE_ATTACHMENT";

// Fallback prefixes (for compatibility)
const FALLBACK_PREFIX: &str = "CLAUDE_CODE_";

/// Environment loader for configuration.
///
/// Loads configuration values from environment variables, supporting
/// both `COCODE_` and `CLAUDE_CODE_` prefixes for compatibility.
#[derive(Debug, Default)]
pub struct EnvLoader;

impl EnvLoader {
    /// Create a new environment loader.
    pub fn new() -> Self {
        Self
    }

    /// Load tool configuration from environment variables.
    pub fn load_tool_config(&self) -> ToolConfig {
        let mut config = ToolConfig::default();

        if let Some(val) = self.get_i32(ENV_MAX_TOOL_CONCURRENCY) {
            config.max_tool_concurrency = val;
            debug!(env = ENV_MAX_TOOL_CONCURRENCY, value = val, "loaded");
        }

        if let Some(val) = self.get_i32(ENV_MCP_TOOL_TIMEOUT) {
            config.mcp_tool_timeout = Some(val);
            debug!(env = ENV_MCP_TOOL_TIMEOUT, value = val, "loaded");
        }

        config
    }

    /// Load compaction configuration from environment variables.
    pub fn load_compact_config(&self) -> CompactConfig {
        let mut config = CompactConfig::default();

        // Feature toggles
        if self.get_bool(ENV_DISABLE_COMPACT) {
            config.disable_compact = true;
            debug!(env = ENV_DISABLE_COMPACT, "loaded");
        }

        if self.get_bool(ENV_DISABLE_AUTO_COMPACT) {
            config.disable_auto_compact = true;
            debug!(env = ENV_DISABLE_AUTO_COMPACT, "loaded");
        }

        if self.get_bool(ENV_DISABLE_MICRO_COMPACT) {
            config.disable_micro_compact = true;
            debug!(env = ENV_DISABLE_MICRO_COMPACT, "loaded");
        }

        // Overrides
        if let Some(val) = self.get_i32(ENV_AUTOCOMPACT_PCT) {
            config.auto_compact_pct = Some(val);
            debug!(env = ENV_AUTOCOMPACT_PCT, value = val, "loaded");
        }

        if let Some(val) = self.get_i32(ENV_BLOCKING_LIMIT) {
            config.blocking_limit_override = Some(val);
            debug!(env = ENV_BLOCKING_LIMIT, value = val, "loaded");
        }

        // Session memory
        if let Some(val) = self.get_i32(ENV_SESSION_MEMORY_MIN) {
            config.session_memory_min_tokens = val;
            debug!(env = ENV_SESSION_MEMORY_MIN, value = val, "loaded");
        }

        if let Some(val) = self.get_i32(ENV_SESSION_MEMORY_MAX) {
            config.session_memory_max_tokens = val;
            debug!(env = ENV_SESSION_MEMORY_MAX, value = val, "loaded");
        }

        if let Some(val) = self.get_i32(ENV_EXTRACTION_COOLDOWN) {
            config.extraction_cooldown_secs = val;
            debug!(env = ENV_EXTRACTION_COOLDOWN, value = val, "loaded");
        }

        // Context restoration
        if let Some(val) = self.get_i32(ENV_CONTEXT_RESTORE_FILES) {
            config.context_restore_max_files = val;
            debug!(env = ENV_CONTEXT_RESTORE_FILES, value = val, "loaded");
        }

        if let Some(val) = self.get_i32(ENV_CONTEXT_RESTORE_BUDGET) {
            config.context_restore_budget = val;
            debug!(env = ENV_CONTEXT_RESTORE_BUDGET, value = val, "loaded");
        }

        if let Some(val) = self.get_i32(ENV_MAX_TOKENS_PER_FILE) {
            config.max_tokens_per_file = val;
            debug!(env = ENV_MAX_TOKENS_PER_FILE, value = val, "loaded");
        }

        // Threshold control
        if let Some(val) = self.get_i32(ENV_MIN_TOKENS_TO_PRESERVE) {
            config.min_tokens_to_preserve = val;
            debug!(env = ENV_MIN_TOKENS_TO_PRESERVE, value = val, "loaded");
        }

        if let Some(val) = self.get_i32(ENV_WARNING_THRESHOLD_OFFSET) {
            config.warning_threshold_offset = val;
            debug!(env = ENV_WARNING_THRESHOLD_OFFSET, value = val, "loaded");
        }

        if let Some(val) = self.get_i32(ENV_ERROR_THRESHOLD_OFFSET) {
            config.error_threshold_offset = val;
            debug!(env = ENV_ERROR_THRESHOLD_OFFSET, value = val, "loaded");
        }

        if let Some(val) = self.get_i32(ENV_MIN_BLOCKING_OFFSET) {
            config.min_blocking_offset = val;
            debug!(env = ENV_MIN_BLOCKING_OFFSET, value = val, "loaded");
        }

        // Micro-compact
        if let Some(val) = self.get_i32(ENV_MICRO_COMPACT_MIN_SAVINGS) {
            config.micro_compact_min_savings = val;
            debug!(env = ENV_MICRO_COMPACT_MIN_SAVINGS, value = val, "loaded");
        }

        if let Some(val) = self.get_i32(ENV_MICRO_COMPACT_THRESHOLD) {
            config.micro_compact_threshold = val;
            debug!(env = ENV_MICRO_COMPACT_THRESHOLD, value = val, "loaded");
        }

        if let Some(val) = self.get_i32(ENV_RECENT_TOOL_RESULTS_TO_KEEP) {
            config.recent_tool_results_to_keep = val;
            debug!(env = ENV_RECENT_TOOL_RESULTS_TO_KEEP, value = val, "loaded");
        }

        // Full compact
        if let Some(val) = self.get_i32(ENV_MAX_SUMMARY_RETRIES) {
            config.max_summary_retries = val;
            debug!(env = ENV_MAX_SUMMARY_RETRIES, value = val, "loaded");
        }

        if let Some(val) = self.get_i32(ENV_MAX_COMPACT_OUTPUT_TOKENS) {
            config.max_compact_output_tokens = val;
            debug!(env = ENV_MAX_COMPACT_OUTPUT_TOKENS, value = val, "loaded");
        }

        if let Some(val) = self.get_f64(ENV_TOKEN_SAFETY_MARGIN) {
            config.token_safety_margin = val;
            debug!(env = ENV_TOKEN_SAFETY_MARGIN, value = val, "loaded");
        }

        if let Some(val) = self.get_i32(ENV_TOKENS_PER_IMAGE) {
            config.tokens_per_image = val;
            debug!(env = ENV_TOKENS_PER_IMAGE, value = val, "loaded");
        }

        config
    }

    /// Load plan mode configuration from environment variables.
    pub fn load_plan_config(&self) -> PlanModeConfig {
        let mut config = PlanModeConfig::default();

        if let Some(val) = self.get_i32(ENV_PLAN_AGENT_COUNT) {
            config.agent_count = val;
            debug!(env = ENV_PLAN_AGENT_COUNT, value = val, "loaded");
        }

        if let Some(val) = self.get_i32(ENV_PLAN_EXPLORE_AGENT_COUNT) {
            config.explore_agent_count = val;
            debug!(env = ENV_PLAN_EXPLORE_AGENT_COUNT, value = val, "loaded");
        }

        // Clamp values to valid range
        config.clamp_all();

        config
    }

    /// Load attachment configuration from environment variables.
    pub fn load_attachment_config(&self) -> AttachmentConfig {
        let mut config = AttachmentConfig::default();

        if self.get_bool(ENV_DISABLE_ATTACHMENTS) {
            config.disable_attachments = true;
            debug!(env = ENV_DISABLE_ATTACHMENTS, "loaded");
        }

        if self.get_bool(ENV_ENABLE_TOKEN_USAGE) {
            config.enable_token_usage_attachment = true;
            debug!(env = ENV_ENABLE_TOKEN_USAGE, "loaded");
        }

        config
    }

    /// Load path configuration from environment variables.
    pub fn load_path_config(&self) -> PathConfig {
        let mut config = PathConfig::default();

        if let Some(val) = self.get_path(ENV_PROJECT_DIR) {
            config.project_dir = Some(val.clone());
            debug!(env = ENV_PROJECT_DIR, path = %val.display(), "loaded");
        }

        if let Some(val) = self.get_path(ENV_PLUGIN_ROOT) {
            config.plugin_root = Some(val.clone());
            debug!(env = ENV_PLUGIN_ROOT, path = %val.display(), "loaded");
        }

        if let Some(val) = self.get_path(ENV_ENV_FILE) {
            config.env_file = Some(val.clone());
            debug!(env = ENV_ENV_FILE, path = %val.display(), "loaded");
        }

        config
    }

    // ============================================================
    // Helper Methods
    // ============================================================

    /// Get a string value from environment, trying both COCODE_ and CLAUDE_CODE_ prefixes.
    fn get_string(&self, key: &str) -> Option<String> {
        env::var(key).ok().or_else(|| {
            // Try fallback prefix
            if let Some(suffix) = key.strip_prefix("COCODE_") {
                let fallback = format!("{FALLBACK_PREFIX}{suffix}");
                env::var(&fallback).ok()
            } else {
                None
            }
        })
    }

    /// Get an i32 value from environment.
    ///
    /// Returns `None` if the variable is not set or cannot be parsed.
    /// Logs a warning if the value is set but cannot be parsed.
    fn get_i32(&self, key: &str) -> Option<i32> {
        self.get_string(key).and_then(|s| match s.parse::<i32>() {
            Ok(v) => Some(v),
            Err(_) => {
                tracing::warn!(key, value = %s, "Failed to parse i32 from env var");
                None
            }
        })
    }

    /// Get an f64 value from environment.
    ///
    /// Returns `None` if the variable is not set or cannot be parsed.
    /// Logs a warning if the value is set but cannot be parsed.
    fn get_f64(&self, key: &str) -> Option<f64> {
        self.get_string(key).and_then(|s| match s.parse::<f64>() {
            Ok(v) => Some(v),
            Err(_) => {
                tracing::warn!(key, value = %s, "Failed to parse f64 from env var");
                None
            }
        })
    }

    /// Get a boolean value from environment.
    ///
    /// Returns true if the environment variable is set to "1", "true", or "yes" (case-insensitive).
    /// Returns false if not set or set to "0", "false", "no", or empty.
    /// Logs a warning if the value is set but not a recognized boolean.
    fn get_bool(&self, key: &str) -> bool {
        self.get_string(key)
            .map(|s| {
                let lower = s.to_lowercase();
                let is_true = matches!(lower.as_str(), "1" | "true" | "yes");
                let is_false = matches!(lower.as_str(), "0" | "false" | "no" | "");
                if !is_true && !is_false {
                    tracing::warn!(key, value = %s, "Unrecognized boolean value, treating as false");
                }
                is_true
            })
            .unwrap_or(false)
    }

    /// Get a path value from environment.
    fn get_path(&self, key: &str) -> Option<PathBuf> {
        self.get_string(key).map(PathBuf::from)
    }
}

#[cfg(test)]
#[path = "env_loader.test.rs"]
mod tests;
