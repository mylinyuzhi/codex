//! Installation tracking for installed plugins.
//!
//! Persists installed plugin metadata to `installed_plugins.json`.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use tracing::debug;
use tracing::warn;

use crate::error::Result;
use crate::error::plugin_error::RegistryCorruptedSnafu;

/// Registry of installed plugins (V2 format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPluginsRegistry {
    /// Schema version (always 2).
    pub version: u32,
    /// Installed plugins keyed by plugin ID, with per-scope entries.
    pub plugins: HashMap<String, Vec<InstalledPluginEntry>>,
}

/// A single installed plugin entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPluginEntry {
    pub scope: String,
    pub version: String,
    pub install_path: PathBuf,
    pub installed_at: String,
    pub last_updated: String,
    pub git_commit_sha: Option<String>,
    pub project_path: Option<PathBuf>,
}

impl InstalledPluginsRegistry {
    /// Create a new empty registry.
    pub fn empty() -> Self {
        Self {
            version: 2,
            plugins: HashMap::new(),
        }
    }

    /// Load registry from disk. Returns empty if missing or corrupt.
    pub fn load(path: &Path) -> Self {
        if !path.exists() {
            return Self::empty();
        }

        match std::fs::read_to_string(path) {
            Ok(content) => match serde_json::from_str::<Self>(&content) {
                Ok(registry) => {
                    debug!(
                        path = %path.display(),
                        plugins = registry.plugins.len(),
                        "Loaded installed plugins registry"
                    );
                    registry
                }
                Err(e) => {
                    warn!(
                        path = %path.display(),
                        error = %e,
                        "Corrupted installed plugins registry, starting fresh"
                    );
                    Self::empty()
                }
            },
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "Failed to read installed plugins registry"
                );
                Self::empty()
            }
        }
    }

    /// Save registry to disk.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                RegistryCorruptedSnafu {
                    path: parent.to_path_buf(),
                    message: format!("Failed to create directory: {e}"),
                }
                .build()
            })?;
        }

        let content = serde_json::to_string_pretty(self).map_err(|e| {
            RegistryCorruptedSnafu {
                path: path.to_path_buf(),
                message: format!("Failed to serialize: {e}"),
            }
            .build()
        })?;

        std::fs::write(path, content).map_err(|e| {
            RegistryCorruptedSnafu {
                path: path.to_path_buf(),
                message: format!("Failed to write: {e}"),
            }
            .build()
        })?;

        debug!(path = %path.display(), "Saved installed plugins registry");
        Ok(())
    }

    /// Add or update a plugin entry.
    pub fn add(&mut self, plugin_id: &str, entry: InstalledPluginEntry) {
        let entries = self.plugins.entry(plugin_id.to_string()).or_default();

        // Replace existing entry with same scope, or append
        if let Some(pos) = entries.iter().position(|e| e.scope == entry.scope) {
            entries[pos] = entry;
        } else {
            entries.push(entry);
        }
    }

    /// Remove a plugin entry by scope.
    pub fn remove(&mut self, plugin_id: &str, scope: &str) -> Option<InstalledPluginEntry> {
        let entries = self.plugins.get_mut(plugin_id)?;
        let pos = entries.iter().position(|e| e.scope == scope)?;
        let removed = entries.remove(pos);

        if entries.is_empty() {
            self.plugins.remove(plugin_id);
        }

        Some(removed)
    }

    /// Get all entries for a plugin.
    pub fn get(&self, plugin_id: &str) -> Option<&[InstalledPluginEntry]> {
        self.plugins.get(plugin_id).map(Vec::as_slice)
    }

    /// Check if there are no installed plugins.
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// Get all plugin IDs.
    pub fn all_plugin_ids(&self) -> Vec<&str> {
        self.plugins.keys().map(String::as_str).collect()
    }
}

#[cfg(test)]
#[path = "installed_registry.test.rs"]
mod tests;
