//! Plugin enable/disable state management.
//!
//! Persists plugin enabled state to `settings.json`.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;
use tracing::debug;
use tracing::warn;

use crate::error::Result;
use crate::error::plugin_error::CacheSnafu;

/// Plugin settings tracking enabled/disabled state and per-plugin configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginSettings {
    /// Map of plugin ID to enabled state.
    #[serde(default)]
    pub enabled_plugins: HashMap<String, bool>,

    /// Per-plugin user configuration.
    ///
    /// Outer key is the plugin ID, inner map is key-value config pairs.
    /// These values can be referenced in MCP configs via `${user_config.KEY}`.
    #[serde(default)]
    pub plugin_config: HashMap<String, HashMap<String, serde_json::Value>>,
}

impl PluginSettings {
    /// Load settings from disk. Returns default if missing.
    pub fn load(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }

        match std::fs::read_to_string(path) {
            Ok(content) => match serde_json::from_str::<Self>(&content) {
                Ok(settings) => {
                    debug!(
                        path = %path.display(),
                        plugins = settings.enabled_plugins.len(),
                        "Loaded plugin settings"
                    );
                    settings
                }
                Err(e) => {
                    warn!(
                        path = %path.display(),
                        error = %e,
                        "Invalid plugin settings, using defaults"
                    );
                    Self::default()
                }
            },
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "Failed to read plugin settings"
                );
                Self::default()
            }
        }
    }

    /// Save settings to disk.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CacheSnafu {
                    path: parent.to_path_buf(),
                    message: format!("Failed to create directory: {e}"),
                }
                .build()
            })?;
        }

        let content = serde_json::to_string_pretty(self).map_err(|e| {
            CacheSnafu {
                path: path.to_path_buf(),
                message: format!("Failed to serialize settings: {e}"),
            }
            .build()
        })?;

        std::fs::write(path, content).map_err(|e| {
            CacheSnafu {
                path: path.to_path_buf(),
                message: format!("Failed to write settings: {e}"),
            }
            .build()
        })?;

        Ok(())
    }

    /// Check if a plugin is enabled. Default is `true` if not explicitly set.
    pub fn is_enabled(&self, plugin_id: &str) -> bool {
        self.enabled_plugins.get(plugin_id).copied().unwrap_or(true)
    }

    /// Set the enabled state for a plugin.
    pub fn set_enabled(&mut self, plugin_id: &str, enabled: bool) {
        self.enabled_plugins.insert(plugin_id.to_string(), enabled);
    }

    /// Remove a plugin from the settings.
    pub fn remove(&mut self, plugin_id: &str) {
        self.enabled_plugins.remove(plugin_id);
        self.plugin_config.remove(plugin_id);
    }

    /// Get a config value for a specific plugin.
    pub fn get_config(&self, plugin_id: &str, key: &str) -> Option<&serde_json::Value> {
        self.plugin_config.get(plugin_id)?.get(key)
    }

    /// Set a config value for a specific plugin.
    pub fn set_config(&mut self, plugin_id: &str, key: &str, value: serde_json::Value) {
        self.plugin_config
            .entry(plugin_id.to_string())
            .or_default()
            .insert(key.to_string(), value);
    }

    /// Get the full config map for a plugin.
    pub fn get_plugin_config(
        &self,
        plugin_id: &str,
    ) -> Option<&HashMap<String, serde_json::Value>> {
        self.plugin_config.get(plugin_id)
    }
}

#[cfg(test)]
#[path = "plugin_settings.test.rs"]
mod tests;
