//! Configuration file storage and resolution layer.
//!
//! This module encapsulates the configuration loading, caching, and resolution
//! logic that was previously mixed into ConfigManager. It provides a clean
//! separation between file I/O and runtime state management.

use crate::error::ConfigError;
use crate::error::config_error::InternalSnafu;
use crate::json_config::AppConfig;
use crate::loader::ConfigLoader;
use crate::resolver::ConfigResolver;
use crate::types::ProviderConfig;
use cocode_protocol::ModelInfo;
use cocode_protocol::ProviderType;
use std::path::Path;
use std::path::PathBuf;
use std::sync::RwLock;
use tracing::debug;
use tracing::info;

/// Encapsulates configuration file storage, loading, and resolution.
///
/// ConfigStore handles:
/// - Loading configuration from files
/// - Caching loaded configurations
/// - Resolver instantiation and updates
/// - Reloading configuration from disk
///
/// This is separate from `RuntimeState` which manages runtime overrides.
#[derive(Debug)]
pub struct ConfigStore {
    /// Path to the configuration directory.
    config_path: PathBuf,
    /// Configuration loader.
    loader: ConfigLoader,
    /// Cached resolver for resolved configurations.
    resolver: RwLock<ConfigResolver>,
    /// Cached application configuration.
    config: RwLock<AppConfig>,
}

impl ConfigStore {
    /// Create a store for the default config directory (~/.cocode).
    ///
    /// Loads configuration files if they exist, otherwise uses built-in defaults.
    pub fn from_default() -> Result<Self, ConfigError> {
        let loader = ConfigLoader::default();
        Self::from_loader(loader)
    }

    /// Create a store for a specific config directory.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let loader = ConfigLoader::from_path(path);
        Self::from_loader(loader)
    }

    /// Create a store from a loader.
    fn from_loader(loader: ConfigLoader) -> Result<Self, ConfigError> {
        let config_path = loader.config_dir().to_path_buf();
        let loaded = loader.load_all()?;

        let resolver =
            ConfigResolver::with_config_dir(loaded.models, loaded.providers, &config_path);

        let config = loaded.config;

        debug!(
            path = %config_path.display(),
            "Loaded configuration"
        );

        Ok(Self {
            config_path,
            loader,
            resolver: RwLock::new(resolver),
            config: RwLock::new(config),
        })
    }

    /// Create an empty store with only built-in defaults.
    pub fn empty() -> Self {
        Self {
            config_path: PathBuf::new(),
            loader: ConfigLoader::from_path(""),
            resolver: RwLock::new(ConfigResolver::empty()),
            config: RwLock::new(AppConfig::default()),
        }
    }

    /// Get the configuration directory path.
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// Reload configuration from disk.
    ///
    /// Updates the cached resolver and config with fresh data from files.
    pub fn reload(&self) -> Result<(), ConfigError> {
        let loaded = self.loader.load_all()?;

        let resolver =
            ConfigResolver::with_config_dir(loaded.models, loaded.providers, &self.config_path);

        // Update cached resolver
        let mut resolver_guard = self.resolver.write().map_err(|e| {
            InternalSnafu {
                message: format!("Failed to acquire write lock: {e}"),
            }
            .build()
        })?;
        *resolver_guard = resolver;

        // Update cached config
        let mut config = self.config.write().map_err(|e| {
            InternalSnafu {
                message: format!("Failed to acquire write lock: {e}"),
            }
            .build()
        })?;
        *config = loaded.config;

        info!("Reloaded configuration");
        Ok(())
    }

    /// Get resolver for queries (internal use).
    ///
    /// This is package-private to prevent direct resolver access outside the config crate.
    pub(crate) fn get_resolver(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<ConfigResolver>, ConfigError> {
        self.resolver.read().map_err(|e| {
            InternalSnafu {
                message: format!("Failed to acquire read lock: {e}"),
            }
            .build()
        })
    }

    /// Get mutable resolver for updates (internal use).
    pub(crate) fn get_resolver_mut(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<ConfigResolver>, ConfigError> {
        self.resolver.write().map_err(|e| {
            InternalSnafu {
                message: format!("Failed to acquire write lock: {e}"),
            }
            .build()
        })
    }

    /// Get app config (internal use).
    pub(crate) fn get_config(&self) -> Result<std::sync::RwLockReadGuard<AppConfig>, ConfigError> {
        self.config.read().map_err(|e| {
            InternalSnafu {
                message: format!("Failed to acquire read lock: {e}"),
            }
            .build()
        })
    }

    /// Get mutable app config (internal use).
    pub(crate) fn get_config_mut(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<AppConfig>, ConfigError> {
        self.config.write().map_err(|e| {
            InternalSnafu {
                message: format!("Failed to acquire write lock: {e}"),
            }
            .build()
        })
    }

    /// Resolve model info with all layers merged (internal use).
    pub(crate) fn resolve_model_info(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<ModelInfo, ConfigError> {
        let resolver = self.get_resolver()?;
        resolver.resolve_model_info(provider, model)
    }

    /// Get provider type by name (internal use).
    pub(crate) fn provider_type(&self, provider: &str) -> Result<ProviderType, ConfigError> {
        let resolver = self.get_resolver()?;
        resolver.provider_type(provider)
    }

    /// Resolve a model alias to its API model name (internal use).
    pub(crate) fn resolve_model_alias(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<String, ConfigError> {
        let resolver = self.get_resolver()?;
        Ok(resolver.resolve_model_alias(provider, model).to_string())
    }

    /// List model slugs for a provider (internal use).
    pub(crate) fn list_model_slugs(&self, provider: &str) -> Result<Vec<String>, ConfigError> {
        let resolver = self.get_resolver()?;
        Ok(resolver
            .list_models(provider)
            .iter()
            .map(|s| s.to_string())
            .collect())
    }

    /// List all available providers (internal use).
    pub(crate) fn list_all_providers(&self) -> Result<Vec<String>, ConfigError> {
        let resolver = self.get_resolver()?;
        Ok(resolver
            .list_providers()
            .iter()
            .map(|s| s.to_string())
            .collect())
    }

    /// Get provider config by name (internal use).
    pub(crate) fn get_provider_config(&self, name: &str) -> Option<ProviderConfig> {
        self.get_resolver()
            .ok()
            .and_then(|r| r.get_provider_config(name).cloned())
    }

    /// Get model config by ID (internal use).
    pub(crate) fn get_model_config(&self, id: &str) -> Option<ModelInfo> {
        self.get_resolver()
            .ok()
            .and_then(|r| r.get_model_config(id).cloned())
    }

    /// Check if a provider exists (internal use).
    pub(crate) fn has_provider(&self, name: &str) -> bool {
        self.get_resolver()
            .ok()
            .map(|r| r.has_provider(name))
            .unwrap_or(false)
    }
}
