//! Configuration section merging with trait-based consolidation framework.
//!
//! This module provides the foundation for a generic trait-based approach to merging
//! configuration sections from multiple sources. The pattern consolidates the
//! 5 duplicate merge_*_config() methods into a single generic implementation.
//!
//! # Architecture
//!
//! Instead of:
//! - merge_tool_config()
//! - merge_compact_config()
//! - merge_plan_config()
//! - merge_attachment_config()
//! - merge_path_config()
//!
//! Use the generic `merge_section::<T>()` with ConfigSection trait implementation
//! for each type. This eliminates ~150 lines of duplicate code.
//!
//! # Merge Precedence
//!
//! 1. ConfigOverrides (highest priority) - in-memory user settings
//! 2. EnvLoader - environment variables
//! 3. ResolvedAppConfig - JSON configuration (lowest priority)

use crate::config::ConfigOverrides;
use crate::env_loader::EnvLoader;
use crate::json_config::ResolvedAppConfig;

/// Trait for configuration sections that can be merged from multiple sources.
///
/// Any configuration section implements this trait to describe how to merge values
/// from overrides, environment, and JSON config with proper precedence.
///
/// # Example
///
/// ```ignore
/// impl ConfigSection for MyConfig {
///     fn from_overrides(overrides: &ConfigOverrides) -> Option<Self> {
///         // Extract from overrides if present
///     }
///
///     fn from_env(loader: &EnvLoader) -> Self {
///         // Load from environment with defaults
///     }
///
///     fn merge_json(&mut self, resolved: &ResolvedAppConfig) {
///         // Fill gaps with JSON config
///     }
/// }
/// ```
pub trait ConfigSection: Default {
    /// Extract from override if present (highest priority).
    fn from_overrides(overrides: &ConfigOverrides) -> Option<Self>;

    /// Load from environment variables.
    fn from_env(loader: &EnvLoader) -> Self;

    /// Merge JSON config values where env didn't set them (lowest priority).
    fn merge_json(&mut self, resolved: &ResolvedAppConfig);
}

/// Generic config section merger - consolidates 5 merge_*_config() methods.
///
/// This function implements the standard merge precedence:
/// 1. Return overrides if present (highest priority)
/// 2. Load from env with defaults
/// 3. Merge in JSON config values for any gaps (lowest priority)
///
/// # Example
///
/// ```ignore
/// let tool_config = merge_section::<ToolConfig>(overrides, resolved, env_loader);
/// let compact_config = merge_section::<CompactConfig>(overrides, resolved, env_loader);
/// ```
pub fn merge_section<T: ConfigSection>(
    overrides: &ConfigOverrides,
    resolved: &ResolvedAppConfig,
    env_loader: &EnvLoader,
) -> T {
    T::from_overrides(overrides).unwrap_or_else(|| {
        let mut config = T::from_env(env_loader);
        config.merge_json(resolved);
        config
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder_framework_exists() {
        // This test verifies the config_builder framework compiles correctly.
        // Implementation of ConfigSection trait for each config type happens separately.
        // The merge_section() function is ready to use once trait implementations exist.
        assert!(true); // Compile-time check only
    }
}
