//! Configuration file loading.
//!
//! This module handles loading configuration from JSON files in the config directory.
//!
//! # Multi-file Support
//!
//! Models and providers support loading from multiple files:
//! - `*model.json` - Model definitions (e.g., `gpt_model.json`, `google_model.json`, `model.json`)
//! - `*provider.json` - Provider configurations (e.g., `test_provider.json`, `provider.json`)
//!
//! Files are loaded in alphabetical order and merged. Duplicate slugs/names are an error.

use crate::error::ConfigError;
use crate::error::config_error::IoSnafu;
use crate::error::config_error::JsonParseSnafu;
use crate::error::config_error::JsoncParseSnafu;
use crate::json_config::AppConfig;
use crate::types::ModelsFile;
use crate::types::ProviderConfig;
use crate::types::ProvidersFile;
use cocode_protocol::ModelInfo;
use jsonc_parser::ParseOptions;
use snafu::ResultExt;
use std::path::Path;
use std::path::PathBuf;
use tracing::debug;

/// Default configuration directory path.
pub const DEFAULT_CONFIG_DIR: &str = ".cocode";

/// Application configuration file name (JSON).
pub const CONFIG_FILE: &str = "config.json";

/// Instruction file names.
pub const AGENTS_MD_FILE: &str = "AGENTS.md";

/// Log directory name.
pub const LOG_DIR_NAME: &str = "log";

/// Environment variable for custom cocode home directory.
pub const COCODE_HOME_ENV: &str = "COCODE_HOME";

/// Environment variable for custom log directory.
pub const COCODE_LOG_DIR_ENV: &str = "COCODE_LOG_DIR";

/// Get the default configuration directory path.
///
/// Returns `~/.cocode` on Unix systems and `%USERPROFILE%\.cocode` on Windows.
pub fn default_config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(DEFAULT_CONFIG_DIR)
}

/// Find the cocode home directory.
///
/// Checks `COCODE_HOME` environment variable first, then falls back to
/// the default config directory (`~/.cocode`).
///
/// If `COCODE_HOME` is a relative path, it's resolved relative to the
/// current working directory.
pub fn find_cocode_home() -> PathBuf {
    if let Ok(custom_home) = std::env::var(COCODE_HOME_ENV) {
        let path = PathBuf::from(&custom_home);
        if path.is_absolute() {
            return path;
        }
        std::env::current_dir()
            .map(|cwd| cwd.join(&custom_home))
            .unwrap_or_else(|_| PathBuf::from(custom_home))
    } else {
        default_config_dir()
    }
}

/// Get the log directory path.
///
/// Checks `COCODE_LOG_DIR` environment variable first, then falls back to
/// `{cocode_home}/log`.
///
/// If `COCODE_LOG_DIR` is a relative path, it's resolved relative to the
/// current working directory.
pub fn log_dir() -> PathBuf {
    if let Ok(custom_log_dir) = std::env::var(COCODE_LOG_DIR_ENV) {
        let path = PathBuf::from(&custom_log_dir);
        if path.is_absolute() {
            return path;
        }
        std::env::current_dir()
            .map(|cwd| cwd.join(&custom_log_dir))
            .unwrap_or_else(|_| PathBuf::from(custom_log_dir))
    } else {
        find_cocode_home().join(LOG_DIR_NAME)
    }
}

/// Load instructions from a project directory.
///
/// Looks for instruction files in the following order:
/// 1. `AGENTS.md`
///
/// Returns `None` if no instruction file is found or if the file is empty.
pub fn load_instructions(project_dir: &Path) -> Option<String> {
    let candidates = [AGENTS_MD_FILE];
    for name in candidates {
        let path = project_dir.join(name);
        if let Ok(content) = std::fs::read_to_string(&path) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Configuration loader for JSON files.
#[derive(Debug, Clone)]
pub struct ConfigLoader {
    config_dir: PathBuf,
}

impl ConfigLoader {
    /// Create a loader for the default config directory (~/.cocode).
    pub fn default() -> Self {
        Self {
            config_dir: default_config_dir(),
        }
    }

    /// Create a loader for a specific config directory.
    pub fn from_path(path: impl AsRef<Path>) -> Self {
        Self {
            config_dir: path.as_ref().to_path_buf(),
        }
    }

    /// Get the config directory path.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Check if the config directory exists.
    pub fn exists(&self) -> bool {
        self.config_dir.exists()
    }

    /// Ensure the config directory exists, creating it if necessary.
    pub fn ensure_dir(&self) -> Result<(), ConfigError> {
        if !self.config_dir.exists() {
            std::fs::create_dir_all(&self.config_dir).context(IoSnafu {
                message: format!(
                    "Failed to create config directory {}",
                    self.config_dir.display(),
                ),
            })?;
            debug!(path = %self.config_dir.display(), "Created config directory");
        }
        Ok(())
    }

    /// Find all config files matching a suffix pattern.
    ///
    /// Returns files matching `*{suffix}.json` in the config directory,
    /// sorted alphabetically for deterministic merge order.
    fn find_config_files(&self, suffix: &str) -> Vec<PathBuf> {
        if !self.config_dir.exists() {
            return Vec::new();
        }

        let pattern = format!("{suffix}.json");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&self.config_dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|name| name.ends_with(&pattern))
            })
            .collect();

        files.sort();
        files
    }

    /// Load models from all `*model.json` files.
    ///
    /// Files are loaded in alphabetical order and merged.
    /// Returns an error if duplicate model slugs are found across files.
    pub fn load_models(&self) -> Result<ModelsFile, ConfigError> {
        let files = self.find_config_files("model");
        if files.is_empty() {
            debug!("No model config files found, using defaults");
            return Ok(ModelsFile::default());
        }

        let mut merged = ModelsFile::default();
        for path in files {
            let list: Vec<ModelInfo> = self.load_json_file(&path)?;
            debug!(path = %path.display(), count = list.len(), "Loaded model file");
            merged.add_models(list, path.display())?;
        }

        Ok(merged)
    }

    /// Load providers from all `*provider.json` files.
    ///
    /// Files are loaded in alphabetical order and merged.
    /// Returns an error if duplicate provider names are found across files.
    pub fn load_providers(&self) -> Result<ProvidersFile, ConfigError> {
        let files = self.find_config_files("provider");
        if files.is_empty() {
            debug!("No provider config files found, using defaults");
            return Ok(ProvidersFile::default());
        }

        let mut merged = ProvidersFile::default();
        for path in files {
            let list: Vec<ProviderConfig> = self.load_json_file(&path)?;
            debug!(path = %path.display(), count = list.len(), "Loaded provider file");
            merged.add_providers(list, path.display())?;
        }

        Ok(merged)
    }

    /// Load the application configuration file (config.json).
    pub fn load_config(&self) -> Result<AppConfig, ConfigError> {
        let path = self.config_dir.join(CONFIG_FILE);
        self.load_json_file(&path)
    }

    /// Load a JSON/JSONC file, returning default if it doesn't exist.
    ///
    /// Supports JSONC extensions:
    /// - Comments (`//` and `/* */`)
    /// - Trailing commas (`[1, 2, 3,]`)
    /// - Unquoted keys (`{key: "value"}`) - only simple alphanumeric names
    fn load_json_file<T: serde::de::DeserializeOwned + Default>(
        &self,
        path: &Path,
    ) -> Result<T, ConfigError> {
        if !path.exists() {
            debug!(path = %path.display(), "Config file not found, using defaults");
            return Ok(T::default());
        }

        let content = std::fs::read_to_string(path).context(IoSnafu {
            message: format!("Failed to read {}", path.display()),
        })?;

        // Handle empty files
        if content.trim().is_empty() {
            debug!(path = %path.display(), "Config file is empty, using defaults");
            return Ok(T::default());
        }

        // Parse with JSONC extensions enabled
        let parse_opts = ParseOptions {
            allow_comments: true,
            allow_trailing_commas: true,
            allow_loose_object_property_names: true,
        };

        let json_value =
            jsonc_parser::parse_to_serde_value(&content, &parse_opts).map_err(|e| {
                JsoncParseSnafu {
                    file: path.display().to_string(),
                    message: e.to_string(),
                }
                .build()
            })?;

        // parse_to_serde_value returns Option<Value>, None means empty/whitespace-only
        let json_value = json_value.unwrap_or(serde_json::Value::Null);

        serde_json::from_value(json_value).context(JsonParseSnafu {
            file: path.display().to_string(),
        })
    }

    /// Load all configuration files at once.
    ///
    /// Returns an error if any configuration file has invalid JSON format or
    /// fails validation (e.g., duplicate provider names). This ensures users
    /// are notified of configuration errors rather than silently using defaults.
    ///
    /// Note: Missing or empty configuration files are handled gracefully by
    /// `load_json_file()` which returns `T::default()` in those cases.
    pub fn load_all(&self) -> Result<LoadedConfig, ConfigError> {
        let models = self.load_models()?;
        let providers = self.load_providers()?;
        let config = self.load_config()?;

        Ok(LoadedConfig {
            models,
            providers,
            config,
        })
    }
}

impl Default for ConfigLoader {
    fn default() -> Self {
        Self::default()
    }
}

/// All loaded configuration data.
#[derive(Debug, Clone, Default)]
pub struct LoadedConfig {
    /// Models configuration (merged from all *model.json files).
    pub models: ModelsFile,
    /// Providers configuration (merged from all *provider.json files).
    pub providers: ProvidersFile,
    /// Application configuration (from config.json).
    pub config: AppConfig,
}

impl LoadedConfig {
    /// Create empty loaded config.
    pub fn empty() -> Self {
        Self::default()
    }
}

#[cfg(test)]
#[path = "loader.test.rs"]
mod tests;
