//! Configuration section merging with trait-based consolidation framework.
//!
//! This module provides a generic trait-based approach to merging configuration
//! sections from multiple sources, consolidating 5 merge methods into a single
//! generic implementation.
//!
//! # Merge Precedence
//!
//! 1. ConfigOverrides (highest priority) - in-memory user settings
//! 2. EnvLoader - environment variables
//! 3. ResolvedAppConfig - JSON configuration (lowest priority)

use crate::config::ConfigOverrides;
use crate::env_loader::EnvLoader;
use crate::json_config::ResolvedAppConfig;
use cocode_protocol::AttachmentConfig;
use cocode_protocol::CompactConfig;
use cocode_protocol::DEFAULT_CONTEXT_RESTORE_BUDGET;
use cocode_protocol::DEFAULT_CONTEXT_RESTORE_MAX_FILES;
use cocode_protocol::DEFAULT_EXTRACTION_COOLDOWN_SECS;
use cocode_protocol::DEFAULT_MAX_TOOL_CONCURRENCY;
use cocode_protocol::DEFAULT_PLAN_AGENT_COUNT;
use cocode_protocol::DEFAULT_PLAN_EXPLORE_AGENT_COUNT;
use cocode_protocol::DEFAULT_SESSION_MEMORY_MAX_TOKENS;
use cocode_protocol::DEFAULT_SESSION_MEMORY_MIN_TOKENS;
use cocode_protocol::MAX_AGENT_COUNT;
use cocode_protocol::MIN_AGENT_COUNT;
use cocode_protocol::PathConfig;
use cocode_protocol::PlanModeConfig;
use cocode_protocol::ToolConfig;

/// Trait for configuration sections that can be merged from multiple sources.
pub trait ConfigSection: Default {
    /// Extract from override if present (highest priority).
    fn from_overrides(overrides: &ConfigOverrides) -> Option<Self>;

    /// Load from environment variables.
    fn from_env(loader: &EnvLoader) -> Self;

    /// Merge JSON config values where env didn't set them (lowest priority).
    fn merge_json(&mut self, resolved: &ResolvedAppConfig);

    /// Post-merge normalization: validation, clamping, etc.
    /// Default: no-op. Override for types needing post-merge processing.
    fn finalize(&mut self) {}
}

/// Generic config section merger.
///
/// Implements the standard merge precedence:
/// 1. Return overrides if present (highest priority)
/// 2. Load from env with defaults
/// 3. Merge in JSON config values for any gaps (lowest priority)
/// 4. Finalize (validation, clamping)
pub fn merge_section<T: ConfigSection>(
    overrides: &ConfigOverrides,
    resolved: &ResolvedAppConfig,
    env_loader: &EnvLoader,
) -> T {
    T::from_overrides(overrides).unwrap_or_else(|| {
        let mut config = T::from_env(env_loader);
        config.merge_json(resolved);
        config.finalize();
        config
    })
}

// ============================================================
// ConfigSection implementations
// ============================================================

impl ConfigSection for ToolConfig {
    fn from_overrides(overrides: &ConfigOverrides) -> Option<Self> {
        overrides.tool_config.clone()
    }

    fn from_env(loader: &EnvLoader) -> Self {
        loader.load_tool_config()
    }

    fn merge_json(&mut self, resolved: &ResolvedAppConfig) {
        if let Some(json_config) = &resolved.tool {
            if self.max_tool_concurrency == DEFAULT_MAX_TOOL_CONCURRENCY {
                self.max_tool_concurrency = json_config.max_tool_concurrency;
            }
            if self.mcp_tool_timeout.is_none() {
                self.mcp_tool_timeout = json_config.mcp_tool_timeout;
            }
        }
    }
}

impl ConfigSection for CompactConfig {
    fn from_overrides(overrides: &ConfigOverrides) -> Option<Self> {
        overrides.compact_config.clone()
    }

    fn from_env(loader: &EnvLoader) -> Self {
        loader.load_compact_config()
    }

    fn merge_json(&mut self, resolved: &ResolvedAppConfig) {
        if let Some(json_config) = &resolved.compact {
            // Boolean fields: OR logic (true from either source wins)
            if !self.disable_compact && json_config.disable_compact {
                self.disable_compact = true;
            }
            if !self.disable_auto_compact && json_config.disable_auto_compact {
                self.disable_auto_compact = true;
            }
            if !self.disable_micro_compact && json_config.disable_micro_compact {
                self.disable_micro_compact = true;
            }
            // Option fields: use JSON if env didn't set
            if self.auto_compact_pct.is_none() {
                self.auto_compact_pct = json_config.auto_compact_pct;
            }
            if self.blocking_limit_override.is_none() {
                self.blocking_limit_override = json_config.blocking_limit_override;
            }
            // Numeric fields: use JSON if env produced the default value
            if self.session_memory_min_tokens == DEFAULT_SESSION_MEMORY_MIN_TOKENS {
                self.session_memory_min_tokens = json_config.session_memory_min_tokens;
            }
            if self.session_memory_max_tokens == DEFAULT_SESSION_MEMORY_MAX_TOKENS {
                self.session_memory_max_tokens = json_config.session_memory_max_tokens;
            }
            if self.extraction_cooldown_secs == DEFAULT_EXTRACTION_COOLDOWN_SECS {
                self.extraction_cooldown_secs = json_config.extraction_cooldown_secs;
            }
            if self.context_restore_max_files == DEFAULT_CONTEXT_RESTORE_MAX_FILES {
                self.context_restore_max_files = json_config.context_restore_max_files;
            }
            if self.context_restore_budget == DEFAULT_CONTEXT_RESTORE_BUDGET {
                self.context_restore_budget = json_config.context_restore_budget;
            }
        }
    }

    fn finalize(&mut self) {
        if let Err(e) = self.validate() {
            tracing::warn!(error = %e, "Invalid compact config");
        }
    }
}

impl ConfigSection for PlanModeConfig {
    fn from_overrides(overrides: &ConfigOverrides) -> Option<Self> {
        overrides.plan_config.clone()
    }

    fn from_env(loader: &EnvLoader) -> Self {
        loader.load_plan_config()
    }

    fn merge_json(&mut self, resolved: &ResolvedAppConfig) {
        if let Some(json_config) = &resolved.plan {
            if self.agent_count == DEFAULT_PLAN_AGENT_COUNT {
                self.agent_count = json_config
                    .agent_count
                    .clamp(MIN_AGENT_COUNT, MAX_AGENT_COUNT);
            }
            if self.explore_agent_count == DEFAULT_PLAN_EXPLORE_AGENT_COUNT {
                self.explore_agent_count = json_config
                    .explore_agent_count
                    .clamp(MIN_AGENT_COUNT, MAX_AGENT_COUNT);
            }
        }
    }

    fn finalize(&mut self) {
        if let Err(e) = self.validate() {
            tracing::warn!(error = %e, "Invalid plan config");
        }
    }
}

impl ConfigSection for AttachmentConfig {
    fn from_overrides(overrides: &ConfigOverrides) -> Option<Self> {
        overrides.attachment_config.clone()
    }

    fn from_env(loader: &EnvLoader) -> Self {
        loader.load_attachment_config()
    }

    fn merge_json(&mut self, resolved: &ResolvedAppConfig) {
        if let Some(json_config) = &resolved.attachment {
            if !self.disable_attachments && json_config.disable_attachments {
                self.disable_attachments = true;
            }
            if !self.enable_token_usage_attachment && json_config.enable_token_usage_attachment {
                self.enable_token_usage_attachment = true;
            }
        }
    }
}

/// Merge PathConfig: overrides or env as base, then always fill gaps from JSON.
///
/// PathConfig differs from the other 4 types: it always merges JSON on top even
/// when overrides are present, so it doesn't use the generic `merge_section()`.
pub fn merge_path_section(
    overrides: &ConfigOverrides,
    resolved: &ResolvedAppConfig,
    env_loader: &EnvLoader,
) -> PathConfig {
    let mut config = overrides
        .path_config
        .clone()
        .unwrap_or_else(|| env_loader.load_path_config());
    if let Some(json_paths) = &resolved.paths {
        if config.project_dir.is_none() {
            config.project_dir = json_paths.project_dir.clone();
        }
        if config.plugin_root.is_none() {
            config.plugin_root = json_paths.plugin_root.clone();
        }
        if config.env_file.is_none() {
            config.env_file = json_paths.env_file.clone();
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_section_returns_override_when_present() {
        let overrides = ConfigOverrides {
            tool_config: Some(ToolConfig {
                max_tool_concurrency: 42,
                ..Default::default()
            }),
            ..Default::default()
        };
        let resolved = ResolvedAppConfig::default();
        let env_loader = EnvLoader::new();

        let config: ToolConfig = merge_section(&overrides, &resolved, &env_loader);
        assert_eq!(config.max_tool_concurrency, 42);
    }

    #[test]
    fn test_merge_section_falls_through_to_env_and_json() {
        let overrides = ConfigOverrides::default();
        let resolved = ResolvedAppConfig::default();
        let env_loader = EnvLoader::new();

        let config: ToolConfig = merge_section(&overrides, &resolved, &env_loader);
        assert_eq!(config.max_tool_concurrency, DEFAULT_MAX_TOOL_CONCURRENCY);
    }
}
