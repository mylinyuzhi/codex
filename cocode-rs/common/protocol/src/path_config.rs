//! Extended path configuration.
//!
//! Defines additional path settings beyond the standard cocode_home and cwd.

use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;

/// Extended path configuration.
///
/// Provides additional path settings for project directory, plugin root,
/// and environment file location.
///
/// # Environment Variables
///
/// - `COCODE_PROJECT_DIR`: Override project directory (usually detected from git root)
/// - `COCODE_PLUGIN_ROOT`: Root directory for plugins/extensions
/// - `COCODE_ENV_FILE`: Path to custom .env file for loading environment variables
///
/// # Example
///
/// ```json
/// {
///   "paths": {
///     "project_dir": "/path/to/project",
///     "plugin_root": "/path/to/plugins",
///     "env_file": "/path/to/.env"
///   }
/// }
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct PathConfig {
    /// Override project directory (usually detected from git root).
    #[serde(default)]
    pub project_dir: Option<PathBuf>,

    /// Root directory for plugins/extensions.
    #[serde(default)]
    pub plugin_root: Option<PathBuf>,

    /// Path to custom .env file for loading environment variables.
    #[serde(default)]
    pub env_file: Option<PathBuf>,
}

impl PathConfig {
    /// Create a new PathConfig with all paths set.
    pub fn new(
        project_dir: Option<PathBuf>,
        plugin_root: Option<PathBuf>,
        env_file: Option<PathBuf>,
    ) -> Self {
        Self {
            project_dir,
            plugin_root,
            env_file,
        }
    }

    /// Check if any paths are configured.
    pub fn is_empty(&self) -> bool {
        self.project_dir.is_none() && self.plugin_root.is_none() && self.env_file.is_none()
    }

    /// Merge another PathConfig into this one.
    ///
    /// Values from `other` override values in `self` if present.
    pub fn merge(&mut self, other: &PathConfig) {
        if other.project_dir.is_some() {
            self.project_dir = other.project_dir.clone();
        }
        if other.plugin_root.is_some() {
            self.plugin_root = other.plugin_root.clone();
        }
        if other.env_file.is_some() {
            self.env_file = other.env_file.clone();
        }
    }
}

#[cfg(test)]
#[path = "path_config.test.rs"]
mod tests;
