//! Enterprise policy for plugin installation.
//!
//! Provides allowlist/blocklist checking for marketplace sources and plugins
//! before installation. Policies can be defined at the organization level
//! to control which plugins are permitted.

use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use tracing::debug;
use tracing::warn;

/// Enterprise policy configuration for plugin management.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginPolicy {
    /// Allowed marketplace sources (if non-empty, only these are permitted).
    #[serde(default)]
    pub allowed_marketplaces: Vec<String>,

    /// Blocked marketplace sources (always denied).
    #[serde(default)]
    pub blocked_marketplaces: Vec<String>,

    /// Allowed plugin names/patterns (if non-empty, only these are permitted).
    #[serde(default)]
    pub allowed_plugins: Vec<String>,

    /// Blocked plugin names/patterns (always denied).
    #[serde(default)]
    pub blocked_plugins: Vec<String>,

    /// Whether plugin installation is completely disabled.
    #[serde(default)]
    pub disable_installation: bool,
}

/// Result of a policy check.
#[derive(Debug, Clone, PartialEq)]
pub enum PolicyDecision {
    /// Installation is allowed.
    Allow,
    /// Installation is denied with a reason.
    Deny(String),
}

impl PluginPolicy {
    /// Load policy from a JSON file.
    ///
    /// Returns default (permissive) policy if the file doesn't exist.
    pub fn load(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }

        match std::fs::read_to_string(path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(policy) => policy,
                Err(e) => {
                    warn!(
                        path = %path.display(),
                        error = %e,
                        "Failed to parse plugin policy, using permissive defaults"
                    );
                    Self::default()
                }
            },
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "Failed to read plugin policy, using permissive defaults"
                );
                Self::default()
            }
        }
    }

    /// Check if a marketplace source is allowed by policy.
    pub fn check_marketplace(&self, marketplace_name: &str) -> PolicyDecision {
        if self.disable_installation {
            return PolicyDecision::Deny(
                "Plugin installation is disabled by enterprise policy".to_string(),
            );
        }

        // Check blocklist first (takes priority)
        if self
            .blocked_marketplaces
            .iter()
            .any(|b| matches_pattern(b, marketplace_name))
        {
            return PolicyDecision::Deny(format!(
                "Marketplace '{marketplace_name}' is blocked by enterprise policy"
            ));
        }

        // Check allowlist (if non-empty, only listed sources are allowed)
        if !self.allowed_marketplaces.is_empty()
            && !self
                .allowed_marketplaces
                .iter()
                .any(|a| matches_pattern(a, marketplace_name))
        {
            return PolicyDecision::Deny(format!(
                "Marketplace '{marketplace_name}' is not in the enterprise allowlist"
            ));
        }

        debug!(
            marketplace = marketplace_name,
            "Marketplace allowed by policy"
        );
        PolicyDecision::Allow
    }

    /// Check if a plugin is allowed by policy.
    pub fn check_plugin(&self, plugin_name: &str) -> PolicyDecision {
        if self.disable_installation {
            return PolicyDecision::Deny(
                "Plugin installation is disabled by enterprise policy".to_string(),
            );
        }

        // Check blocklist first
        if self
            .blocked_plugins
            .iter()
            .any(|b| matches_pattern(b, plugin_name))
        {
            return PolicyDecision::Deny(format!(
                "Plugin '{plugin_name}' is blocked by enterprise policy"
            ));
        }

        // Check allowlist
        if !self.allowed_plugins.is_empty()
            && !self
                .allowed_plugins
                .iter()
                .any(|a| matches_pattern(a, plugin_name))
        {
            return PolicyDecision::Deny(format!(
                "Plugin '{plugin_name}' is not in the enterprise allowlist"
            ));
        }

        debug!(plugin = plugin_name, "Plugin allowed by policy");
        PolicyDecision::Allow
    }

    /// Check if the policy is permissive (no restrictions).
    pub fn is_permissive(&self) -> bool {
        !self.disable_installation
            && self.allowed_marketplaces.is_empty()
            && self.blocked_marketplaces.is_empty()
            && self.allowed_plugins.is_empty()
            && self.blocked_plugins.is_empty()
    }
}

/// Simple pattern matching for policy rules.
///
/// Supports:
/// - Exact match: `"my-plugin"` matches `"my-plugin"`
/// - Prefix wildcard: `"my-*"` matches `"my-plugin"`, `"my-other"`
/// - Suffix wildcard: `"*-plugin"` matches `"my-plugin"`
/// - Any: `"*"` matches everything
fn matches_pattern(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return name.starts_with(prefix);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return name.ends_with(suffix);
    }
    pattern == name
}

/// Default policy file path within the plugins directory.
pub fn policy_path(plugins_dir: &Path) -> PathBuf {
    plugins_dir.join("policy.json")
}

#[cfg(test)]
#[path = "policy.test.rs"]
mod tests;
