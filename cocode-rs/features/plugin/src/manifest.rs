//! Plugin manifest parsing.
//!
//! Each plugin contains a `plugin.json` manifest that declares its metadata
//! and contributions.

use std::collections::HashMap;

use crate::contribution::PluginContributions;
use crate::error::Result;
use crate::error::plugin_error::InvalidManifestSnafu;
use crate::error::plugin_error::IoSnafu;
use crate::error::plugin_error::ManifestNotFoundSnafu;

use serde::Deserialize;
use serde::Serialize;
use std::fs;
use std::path::Path;

/// The expected manifest file name.
pub const PLUGIN_JSON: &str = "plugin.json";

/// Check if a version string is valid semver format.
///
/// Validates basic MAJOR.MINOR.PATCH format with optional prerelease suffix.
/// Examples: "1.0.0", "2.3.1", "1.0.0-beta.1", "0.1.0-alpha+build"
fn is_valid_semver(version: &str) -> bool {
    let parts: Vec<&str> = version.split('-').collect();
    let version_part = parts.first().unwrap_or(&"");

    // Split on '+' to handle build metadata
    let version_part = version_part.split('+').next().unwrap_or("");

    // Must have exactly 3 numeric parts
    let nums: Vec<&str> = version_part.split('.').collect();
    if nums.len() != 3 {
        return false;
    }

    // Each part must be a valid non-negative integer
    for num in nums {
        if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        // No leading zeros allowed (except for "0" itself)
        if num.len() > 1 && num.starts_with('0') {
            return false;
        }
    }

    true
}

/// Plugin manifest as defined in `plugin.json`.
///
/// # Example
///
/// ```json
/// {
///   "plugin": {
///     "name": "my-plugin",
///     "version": "0.1.0",
///     "description": "My custom plugin",
///     "author": "Author Name"
///   },
///   "contributions": {
///     "skills": ["skills/"],
///     "hooks": ["hooks.json"]
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Core plugin metadata.
    pub plugin: PluginMetadata,

    /// Plugin contributions (skills, hooks, agents).
    #[serde(default)]
    pub contributions: PluginContributions,

    /// User-configurable settings schema for this plugin.
    ///
    /// Keys are config field names, values describe defaults or types.
    /// Users can override these via `PluginSettings::set_config()`.
    #[serde(default)]
    pub user_config: HashMap<String, UserConfigField>,
}

/// A user-configurable field declared by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfigField {
    /// Human-readable description of this config field.
    #[serde(default)]
    pub description: Option<String>,

    /// Default value.
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

/// Core plugin metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// Unique plugin name (used as identifier).
    pub name: String,

    /// Plugin version (semver).
    pub version: String,

    /// Human-readable description.
    pub description: String,

    /// Plugin author.
    #[serde(default)]
    pub author: Option<String>,

    /// Repository URL.
    #[serde(default)]
    pub repository: Option<String>,

    /// License identifier.
    #[serde(default)]
    pub license: Option<String>,

    /// Minimum cocode version required.
    #[serde(default)]
    pub min_cocode_version: Option<String>,
}

impl PluginManifest {
    /// Load a plugin manifest from a directory.
    ///
    /// Looks for `plugin.json` in the given directory.
    pub fn from_dir(dir: &Path) -> Result<Self> {
        let manifest_path = dir.join(PLUGIN_JSON);

        if !manifest_path.exists() {
            return Err(ManifestNotFoundSnafu {
                path: manifest_path,
            }
            .build());
        }

        Self::from_file(&manifest_path)
    }

    /// Load a plugin manifest from a file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path).map_err(|e| {
            IoSnafu {
                path: path.to_path_buf(),
                message: e.to_string(),
            }
            .build()
        })?;

        Self::from_str(&content, path)
    }

    /// Parse a plugin manifest from a JSON string.
    pub fn from_str(content: &str, path: &Path) -> Result<Self> {
        serde_json::from_str(content).map_err(|e| {
            InvalidManifestSnafu {
                path: path.to_path_buf(),
                message: e.to_string(),
            }
            .build()
        })
    }

    /// Validate the manifest.
    pub fn validate(&self) -> std::result::Result<(), Vec<String>> {
        let mut errors = Vec::new();

        // Name validation
        if self.plugin.name.is_empty() {
            errors.push("Plugin name cannot be empty".to_string());
        } else if self.plugin.name.len() > 64 {
            errors.push("Plugin name too long (max 64 chars)".to_string());
        } else if !self
            .plugin
            .name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            errors.push(
                "Plugin name can only contain lowercase alphanumeric characters and hyphens"
                    .to_string(),
            );
        }

        // Version validation (semver format: MAJOR.MINOR.PATCH with optional prerelease)
        if self.plugin.version.is_empty() {
            errors.push("Plugin version cannot be empty".to_string());
        } else if !is_valid_semver(&self.plugin.version) {
            errors.push(format!(
                "Plugin version '{}' is not valid semver (expected MAJOR.MINOR.PATCH)",
                self.plugin.version
            ));
        }

        // Description validation
        if self.plugin.description.is_empty() {
            errors.push("Plugin description cannot be empty".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
#[path = "manifest.test.rs"]
mod tests;
