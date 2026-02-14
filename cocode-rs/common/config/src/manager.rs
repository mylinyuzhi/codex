//! Configuration manager with caching and runtime switching.
//!
//! `ConfigManager` is the main entry point for configuration management.
//! It handles loading, caching, and runtime switching of providers and models.

use crate::builtin;
use crate::config::Config;
use crate::config::ConfigOverrides;
use crate::env_loader::EnvLoader;
use crate::error::ConfigError;
use crate::error::NotFoundKind;
use crate::error::config_error::InternalSnafu;
use crate::error::config_error::NotFoundSnafu;
use crate::json_config::AppConfig;
use crate::json_config::LoggingConfig;
use crate::loader::ConfigLoader;
use crate::loader::load_instructions;
use crate::resolver::ConfigResolver;
use crate::types::ModelSummary;
use crate::types::ProviderSummary;
use cocode_protocol::Features;
use cocode_protocol::ModelInfo;
use cocode_protocol::ProviderInfo;
use cocode_protocol::ProviderType;
use cocode_protocol::RoleSelection;
use cocode_protocol::RoleSelections;
use cocode_protocol::SandboxMode;
use cocode_protocol::ThinkingLevel;
use cocode_protocol::model::ModelRole;
use cocode_protocol::model::ModelSpec;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::RwLock;
use tracing::debug;
use tracing::info;

/// Configuration manager for multi-provider setup.
///
/// Provides thread-safe configuration management with:
/// - Lazy loading from JSON and TOML files
/// - Caching with manual reload
/// - Runtime provider/model switching via `ModelSpec`
/// - Layered resolution: Runtime > JSON Config > Built-in Defaults
///
/// # Example
///
/// ```no_run
/// use cocode_config::ConfigManager;
/// use cocode_config::error::ConfigError;
/// use cocode_protocol::model::ModelSpec;
///
/// # fn example() -> Result<(), ConfigError> {
/// // Load from default path (~/.cocode)
/// let manager = ConfigManager::from_default()?;
///
/// // Get current provider/model
/// let spec = manager.current_spec();
/// println!("Current: {}/{}", spec.provider, spec.model);
///
/// // Switch to a different model
/// let new_spec = ModelSpec::new("anthropic", "claude-sonnet-4-20250514");
/// manager.switch_spec(&new_spec)?;
///
/// // Get resolved model info
/// let info = manager.resolve_model_info("anthropic", "claude-sonnet-4-20250514")?;
/// println!("Context window: {:?}", info.context_window);
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct ConfigManager {
    /// Path to the configuration directory.
    config_path: PathBuf,
    /// Configuration loader.
    loader: ConfigLoader,
    /// Cached resolver.
    resolver: RwLock<ConfigResolver>,
    /// Application configuration (from config.json).
    config: RwLock<AppConfig>,
    /// Runtime role selections (highest precedence, in-memory only).
    runtime_selections: RwLock<RoleSelections>,
}

impl ConfigManager {
    /// Create a manager for the default config directory (~/.cocode).
    ///
    /// Loads configuration files if they exist, otherwise uses built-in defaults.
    pub fn from_default() -> Result<Self, ConfigError> {
        let loader = ConfigLoader::default();
        Self::from_loader(loader)
    }

    /// Create a manager for a specific config directory.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let loader = ConfigLoader::from_path(path);
        Self::from_loader(loader)
    }

    /// Create a manager from a loader.
    fn from_loader(loader: ConfigLoader) -> Result<Self, ConfigError> {
        // Ensure built-in defaults are initialized
        builtin::ensure_initialized();

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
            runtime_selections: RwLock::new(RoleSelections::default()),
        })
    }

    /// Create an empty manager with only built-in defaults.
    pub fn empty() -> Self {
        builtin::ensure_initialized();

        Self {
            config_path: PathBuf::new(),
            loader: ConfigLoader::from_path(""),
            resolver: RwLock::new(ConfigResolver::empty()),
            config: RwLock::new(AppConfig::default()),
            runtime_selections: RwLock::new(RoleSelections::default()),
        }
    }

    /// Get the configuration directory path.
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// Resolve model info with all layers merged.
    pub fn resolve_model_info(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<ModelInfo, ConfigError> {
        let resolver = self.read_resolver()?;
        resolver.resolve_model_info(provider, model)
    }

    /// Get the provider type by name (O(1) HashMap lookup, no resolution).
    ///
    /// This is much cheaper than `resolve_provider()` when you only need
    /// the `ProviderType` — it skips API key validation and model resolution.
    pub fn provider_type(&self, provider: &str) -> Result<ProviderType, ConfigError> {
        let resolver = self.read_resolver()?;
        resolver.provider_type(provider)
    }

    /// Resolve a model alias to its API model name (O(1) lookup, no resolution).
    ///
    /// Returns the alias if set and non-empty, otherwise returns the slug.
    /// The result is an owned `String` because the resolver is behind a `RwLock`.
    pub fn resolve_model_alias(&self, provider: &str, model: &str) -> Result<String, ConfigError> {
        let resolver = self.read_resolver()?;
        Ok(resolver.resolve_model_alias(provider, model).to_string())
    }

    /// List model slugs configured for a provider (O(1) lookup, no resolution).
    ///
    /// This is cheaper than `list_models()` which resolves each model's full
    /// `ModelInfo`. Use this when you only need the slug strings.
    pub(crate) fn list_model_slugs(&self, provider: &str) -> Result<Vec<String>, ConfigError> {
        let resolver = self.read_resolver()?;
        Ok(resolver
            .list_models(provider)
            .into_iter()
            .map(String::from)
            .collect())
    }

    /// Resolve provider configuration into a complete `ProviderInfo`.
    ///
    /// The returned `ProviderInfo` contains:
    /// - Resolved API key (from env or config)
    /// - All connection settings (base_url, streaming, wire_api)
    /// - Map of resolved models (slug -> ModelInfo)
    pub fn resolve_provider(&self, provider: &str) -> Result<ProviderInfo, ConfigError> {
        let resolver = self.read_resolver()?;
        resolver.resolve_provider(provider)
    }

    /// Resolve provider using pre-resolved model cache (avoids redundant model resolution).
    ///
    /// This is used by `build_config()` to avoid re-resolving models that were already
    /// resolved in the role resolution phase. The cache is keyed by `ModelSpec`.
    pub(crate) fn resolve_provider_with_cache(
        &self,
        provider: &str,
        model_cache: &HashMap<ModelSpec, ModelInfo>,
    ) -> Result<ProviderInfo, ConfigError> {
        let resolver = self.read_resolver()?;
        resolver.resolve_provider_with_cache(provider, model_cache)
    }

    /// Get the current active provider and model as a typed `ModelSpec`.
    ///
    /// Resolution order (highest to lowest precedence):
    /// 1. Runtime overrides (set via `switch_spec()`)
    /// 2. JSON config with profile resolution (`config.json`)
    /// 3. Built-in defaults ("openai", "gpt-5")
    pub fn current_spec(&self) -> ModelSpec {
        self.current_spec_for_role(ModelRole::Main)
    }

    /// Get the current active provider and model for a specific role as a `ModelSpec`.
    ///
    /// Resolution order (highest to lowest precedence):
    /// 1. Runtime overrides (per-role selections)
    /// 2. JSON config with profile resolution (`config.json`)
    /// 3. Built-in defaults ("openai", "gpt-5")
    pub fn current_spec_for_role(&self, role: ModelRole) -> ModelSpec {
        // 1. Check runtime overrides first (supports all roles)
        if let Ok(runtime) = self.read_runtime() {
            if let Some(selection) = runtime.get_or_main(role) {
                return selection.model.clone();
            }
        }

        // 2. Check JSON config (with profile resolution)
        if let Ok(config) = self.read_config() {
            let resolved = config.resolve();
            if let Some(spec) = resolved.models.get(role) {
                return spec.clone();
            }
        }

        // 3. Fallback to built-in default
        ModelSpec::new("openai", "gpt-5")
    }

    /// Switch to a specific provider and model using a typed `ModelSpec`.
    ///
    /// This updates the runtime overrides (in-memory only).
    /// To persist, edit `config.toml` directly.
    pub fn switch_spec(&self, spec: &ModelSpec) -> Result<(), ConfigError> {
        // Validate the provider
        self.validate_provider(&spec.provider)?;

        // Update runtime overrides (in-memory)
        let mut runtime = self.write_runtime()?;
        runtime.set(ModelRole::Main, RoleSelection::new(spec.clone()));

        info!(provider = %spec.provider, model = %spec.model, "Switched to new model");
        Ok(())
    }

    /// Switch model for a specific role using a typed `ModelSpec`.
    ///
    /// This updates the runtime overrides for the specified role.
    /// To persist, edit the config file directly.
    pub fn switch_role_spec(&self, role: ModelRole, spec: &ModelSpec) -> Result<(), ConfigError> {
        // Validate the provider
        self.validate_provider(&spec.provider)?;

        // Update runtime selections
        let mut runtime = self.write_runtime()?;
        runtime.set(role, RoleSelection::new(spec.clone()));

        info!(
            provider = %spec.provider,
            model = %spec.model,
            role = %role,
            "Switched model for role"
        );
        Ok(())
    }

    /// Switch model and thinking level for a specific role using a typed `ModelSpec`.
    ///
    /// This updates the runtime overrides for the specified role with
    /// both model and thinking level.
    pub fn switch_role_spec_with_thinking(
        &self,
        role: ModelRole,
        spec: &ModelSpec,
        thinking_level: ThinkingLevel,
    ) -> Result<(), ConfigError> {
        // Validate the provider
        self.validate_provider(&spec.provider)?;

        // Update runtime overrides
        let mut runtime = self.write_runtime()?;
        runtime.set(
            role,
            RoleSelection::with_thinking(spec.clone(), thinking_level.clone()),
        );

        info!(
            provider = %spec.provider,
            model = %spec.model,
            role = %role,
            thinking = %thinking_level,
            "Switched model with thinking level for role"
        );
        Ok(())
    }

    /// Get the application configuration.
    pub fn app_config(&self) -> AppConfig {
        self.read_config().map(|c| c.clone()).unwrap_or_default()
    }

    /// Set the active profile (in-memory only).
    ///
    /// This overrides the profile selection from config.json. The change is
    /// in-memory only and will be lost on reload or restart.
    ///
    /// Returns `Ok(true)` if the profile exists, `Ok(false)` if the profile
    /// doesn't exist (profile will still be set, but won't have any effect).
    pub fn set_profile(&self, profile: &str) -> Result<bool, ConfigError> {
        let mut config = self.config.write().map_err(|e| {
            InternalSnafu {
                message: format!("Failed to acquire write lock: {e}"),
            }
            .build()
        })?;

        let exists = config.has_profile(profile);
        config.profile = Some(profile.to_string());

        if exists {
            info!(profile, "Profile set");
        } else {
            info!(
                profile,
                "Profile set (profile not found in config, will use defaults)"
            );
        }

        Ok(exists)
    }

    /// Get the currently active profile name.
    pub fn current_profile(&self) -> Option<String> {
        self.read_config().ok()?.profile.clone()
    }

    /// List all available profiles.
    pub fn list_profiles(&self) -> Vec<String> {
        self.read_config()
            .ok()
            .map(|c| c.list_profiles().into_iter().map(String::from).collect())
            .unwrap_or_default()
    }

    /// Get the logging configuration from config.json.
    ///
    /// Returns `None` if no logging section is configured.
    pub fn logging_config(&self) -> Option<LoggingConfig> {
        self.read_config().ok()?.logging.clone()
    }

    /// Get the current features configuration.
    ///
    /// Combines default features with config overrides and profile overrides.
    pub fn features(&self) -> Features {
        self.read_config()
            .map(|c| c.resolve().features)
            .unwrap_or_else(|_| Features::with_defaults())
    }

    /// Check if a specific feature is enabled.
    ///
    /// Uses the layered features configuration.
    pub fn is_feature_enabled(&self, feature: cocode_protocol::Feature) -> bool {
        self.features().enabled(feature)
    }

    /// Switch only the thinking level for a specific role (in-memory only).
    ///
    /// Keeps the current model but updates the thinking level.
    /// Returns `Ok(false)` if no model is configured for this role.
    pub fn switch_thinking_level(
        &self,
        role: ModelRole,
        thinking_level: ThinkingLevel,
    ) -> Result<bool, ConfigError> {
        let mut runtime = self.write_runtime()?;

        let updated = runtime.set_thinking_level(role, thinking_level.clone());

        if updated {
            info!(
                role = %role,
                thinking = %thinking_level,
                "Switched thinking level for role"
            );
        }

        Ok(updated)
    }

    /// Build a `RoleSelection` for a provider/model pair, resolving provider_type
    /// and the model's default thinking level from `ModelInfo`.
    pub fn resolve_selection(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<RoleSelection, ConfigError> {
        let provider_type = self.provider_type(provider)?;
        let mut spec = ModelSpec::with_type(provider, provider_type, model);
        let model_info = self.resolve_model_info(provider, model).ok();

        if let Some(ref info) = model_info {
            spec.display_name = info.display_name_or_slug().to_string();
        }

        let mut selection = match model_info
            .as_ref()
            .and_then(|i| i.default_thinking_level.clone())
        {
            Some(level) => RoleSelection::with_thinking(spec, level),
            None => RoleSelection::new(spec),
        };

        if let Some(info) = model_info {
            selection.supported_thinking_levels = info.supported_thinking_levels;
        }

        Ok(selection)
    }

    /// Build a `RoleSelection` for the current main model.
    pub fn current_main_selection(&self) -> RoleSelection {
        let spec = self.current_spec();
        self.resolve_selection(&spec.provider, &spec.model)
            .unwrap_or_else(|_| RoleSelection::new(spec))
    }

    /// Build `RoleSelection`s for all configured models across all providers.
    ///
    /// Used for the model picker. Acquires the resolver lock once to avoid
    /// O(n*m) lock acquisitions.
    pub fn all_model_selections(&self) -> Vec<RoleSelection> {
        let Ok(resolver) = self.read_resolver() else {
            return Vec::new();
        };

        let mut selections = Vec::new();
        for provider_name in resolver.list_providers() {
            let provider_type = match resolver.provider_type(provider_name) {
                Ok(pt) => pt,
                Err(_) => continue,
            };
            for slug in resolver.list_models(provider_name) {
                let model_info = resolver.resolve_model_info(provider_name, slug).ok();

                let mut spec = ModelSpec::with_type(provider_name, provider_type, slug);
                if let Some(ref info) = model_info {
                    spec.display_name = info.display_name_or_slug().to_string();
                }

                let mut selection = match model_info
                    .as_ref()
                    .and_then(|i| i.default_thinking_level.clone())
                {
                    Some(level) => RoleSelection::with_thinking(spec, level),
                    None => RoleSelection::new(spec),
                };

                if let Some(info) = model_info {
                    selection.supported_thinking_levels = info.supported_thinking_levels;
                }

                selections.push(selection);
            }
        }
        selections
    }

    /// Get current selection for a role.
    ///
    /// Returns the runtime override selection if set, or None if not overridden.
    pub fn current_selection(&self, role: ModelRole) -> Option<RoleSelection> {
        let runtime = self.read_runtime().ok()?;
        runtime.get(role).cloned()
    }

    /// Get all current runtime selections.
    pub fn current_selections(&self) -> RoleSelections {
        self.read_runtime().map(|r| r.clone()).unwrap_or_default()
    }

    /// Reload configuration from disk.
    ///
    /// This reloads all configuration files (JSON) and updates the cached state.
    /// Note: Runtime overrides are preserved across reloads.
    ///
    /// For empty managers (created via `empty()`), this is a no-op.
    pub fn reload(&self) -> Result<(), ConfigError> {
        // Empty managers have no config files to reload
        if self.config_path.as_os_str().is_empty() {
            debug!("Skipping reload for empty manager (no config path)");
            return Ok(());
        }

        let loaded = self.loader.load_all()?;

        let new_resolver =
            ConfigResolver::with_config_dir(loaded.models, loaded.providers, &self.config_path);

        {
            let mut resolver = self.resolver.write().map_err(|e| {
                InternalSnafu {
                    message: format!("Failed to acquire write lock: {e}"),
                }
                .build()
            })?;
            *resolver = new_resolver;
        }

        {
            let mut config = self.config.write().map_err(|e| {
                InternalSnafu {
                    message: format!("Failed to acquire write lock: {e}"),
                }
                .build()
            })?;
            *config = loaded.config;
        }

        info!("Reloaded configuration");
        Ok(())
    }

    /// List all available providers.
    ///
    /// Returns providers from both configuration files and built-in defaults.
    pub fn list_providers(&self) -> Vec<ProviderSummary> {
        let Ok(resolver) = self.read_resolver() else {
            return Vec::new();
        };
        let mut summaries = Vec::new();

        // Add configured providers
        for name in resolver.list_providers() {
            if let Some(config) = resolver.get_provider_config(name) {
                summaries.push(ProviderSummary::from_config(name, config));
            }
        }

        // Add built-in providers not already in config
        for name in builtin::list_builtin_providers() {
            if !summaries.iter().any(|s| s.name == name) {
                if let Some(config) = builtin::get_provider_defaults(name) {
                    summaries.push(ProviderSummary::from_builtin(name, &config));
                }
            }
        }

        summaries
    }

    /// List models for a specific provider.
    ///
    /// Returns models from both configuration files and built-in defaults.
    pub fn list_models(&self, provider: &str) -> Vec<ModelSummary> {
        let Ok(resolver) = self.read_resolver() else {
            return Vec::new();
        };
        let mut summaries = Vec::new();

        // Add configured models for this provider
        for model_id in resolver.list_models(provider) {
            if let Ok(info) = resolver.resolve_model_info(provider, model_id) {
                summaries.push(ModelSummary::from_model_info(model_id, &info));
            }
        }

        // If no models configured, suggest some built-in ones based on provider type
        if summaries.is_empty() {
            if let Some(provider_config) = resolver.get_provider_config(provider) {
                let suggested = suggest_models_for_provider(provider_config.provider_type);
                for model_id in suggested {
                    if let Some(info) = builtin::get_model_defaults(model_id) {
                        summaries.push(ModelSummary::from_model_info(model_id, &info));
                    }
                }
            }
        }

        summaries
    }

    /// Build a complete Config snapshot from current state.
    ///
    /// This method creates a complete runtime configuration snapshot that includes:
    /// - All resolved model roles
    /// - All available providers with resolved API keys
    /// - Features from config with defaults applied
    /// - Paths (cwd, cocode_home)
    /// - User instructions from AGENTS.md
    /// - Sandbox configuration
    ///
    /// # Example
    ///
    /// ```no_run
    /// use cocode_config::{ConfigManager, ConfigOverrides};
    /// use cocode_protocol::model::ModelRole;
    ///
    /// # fn example() -> Result<(), cocode_config::error::ConfigError> {
    /// let manager = ConfigManager::from_default()?;
    /// let config = manager.build_config(ConfigOverrides::default())?;
    ///
    /// // Access main model
    /// if let Some(main) = config.main_model_info() {
    ///     println!("Main: {} ({:?})", main.display_name_or_slug(), main.context_window);
    /// }
    ///
    /// // Access fast model (falls back to main if not configured)
    /// if let Some(fast) = config.model_for_role(ModelRole::Fast) {
    ///     println!("Fast: {}", fast.display_name_or_slug());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn build_config(&self, overrides: ConfigOverrides) -> Result<Config, ConfigError> {
        // Get resolved app config (with profile applied)
        let app_config = self.read_config()?;
        let resolved = app_config.resolve();

        // Merge model overrides
        let mut models = resolved.models.clone();
        if let Some(override_models) = &overrides.models {
            models.merge(override_models);
        }

        // Build model cache and providers
        let (resolved_models, providers) = self.build_model_cache(&models)?;

        // Resolve cwd and instructions
        let cwd = overrides
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let user_instructions = overrides
            .user_instructions
            .as_ref()
            .cloned()
            .or_else(|| load_instructions(&cwd));

        // Merge features with overrides
        let mut features = resolved.features.clone();
        if !overrides.features.is_empty() {
            features.apply_map(
                &overrides
                    .features
                    .iter()
                    .map(|(k, v)| (k.clone(), *v))
                    .collect(),
            );
        }

        // Build sandbox config
        let sandbox_mode = overrides.sandbox_mode.unwrap_or_default();
        let writable_roots = overrides
            .writable_roots
            .as_ref()
            .cloned()
            .unwrap_or_else(|| {
                if sandbox_mode == SandboxMode::WorkspaceWrite {
                    vec![cwd.clone()]
                } else {
                    Vec::new()
                }
            });

        // Load extended configs from environment variables
        let env_loader = EnvLoader::new();

        // Merge individual config sections
        use crate::config_builder::merge_path_section;
        use crate::config_builder::merge_section;
        let tool_config =
            merge_section::<cocode_protocol::ToolConfig>(&overrides, &resolved, &env_loader);
        let mut compact_config =
            merge_section::<cocode_protocol::CompactConfig>(&overrides, &resolved, &env_loader);
        if let Some(main_info) = resolved_models.get(&ModelRole::Main) {
            compact_config.apply_model_overrides(main_info);
        }
        let plan_config =
            merge_section::<cocode_protocol::PlanModeConfig>(&overrides, &resolved, &env_loader);
        let attachment_config =
            merge_section::<cocode_protocol::AttachmentConfig>(&overrides, &resolved, &env_loader);
        let path_config = merge_path_section(&overrides, &resolved, &env_loader);

        // Web search and fetch configs
        let web_search_config = resolved.web_search.clone().unwrap_or_default();
        let web_fetch_config = resolved.web_fetch.clone().unwrap_or_default();

        // OTel config: resolve from JSON + env vars
        let otel = resolved
            .otel
            .as_ref()
            .filter(|c| c.is_enabled())
            .map(|c| c.to_otel_settings(&self.config_path));

        Ok(Config {
            models,
            providers,
            resolved_models,
            cwd,
            cocode_home: self.config_path.clone(),
            user_instructions,
            features,
            logging: resolved.logging,
            active_profile: app_config.profile.clone(),
            ephemeral: overrides.ephemeral.unwrap_or(false),
            sandbox_mode,
            writable_roots,
            tool_config,
            compact_config,
            plan_config,
            attachment_config,
            path_config,
            web_search_config,
            web_fetch_config,
            permissions: resolved.permissions.clone(),
            hooks: resolved.hooks.clone(),
            otel,
            output_style: resolved.output_style.clone(),
        })
    }

    // Private helper methods for config building

    /// Build resolved models and providers from configured model roles.
    ///
    /// Returns (resolved_models_by_role, providers).
    fn build_model_cache(
        &self,
        models: &cocode_protocol::ModelRoles,
    ) -> Result<(HashMap<ModelRole, ModelInfo>, HashMap<String, ProviderInfo>), ConfigError> {
        use crate::model_cache::ModelCache;

        // Cache provider list (used twice: model collection + provider building)
        let provider_names: Vec<String> = self
            .list_providers()
            .iter()
            .map(|s| s.name.clone())
            .collect();

        // Phases 1-3: collect, resolve, build role lookups (delegated to ModelCache)
        let mut cache = ModelCache::new();
        let resolved_models = cache.build_for_roles(
            models,
            || provider_names.clone(),
            |p| self.list_model_slugs(p),
            |p, m| self.resolve_model_info(p, m),
        )?;

        // Phase 4: build providers using cached resolutions
        let model_cache = cache.into_inner();
        let mut providers = HashMap::new();
        for name in &provider_names {
            if let Ok(info) = self.resolve_provider_with_cache(name, &model_cache) {
                providers.insert(name.clone(), info);
            }
        }

        Ok((resolved_models, providers))
    }

    // Private helper methods for lock management

    /// Acquire read lock on resolver.
    fn read_resolver(&self) -> Result<std::sync::RwLockReadGuard<'_, ConfigResolver>, ConfigError> {
        self.resolver.read().map_err(|e| {
            InternalSnafu {
                message: format!("Failed to acquire resolver read lock: {e}"),
            }
            .build()
        })
    }

    /// Acquire read lock on config.
    fn read_config(&self) -> Result<std::sync::RwLockReadGuard<'_, AppConfig>, ConfigError> {
        self.config.read().map_err(|e| {
            InternalSnafu {
                message: format!("Failed to acquire config read lock: {e}"),
            }
            .build()
        })
    }

    /// Acquire read lock on runtime selections.
    fn read_runtime(&self) -> Result<std::sync::RwLockReadGuard<'_, RoleSelections>, ConfigError> {
        self.runtime_selections.read().map_err(|e| {
            InternalSnafu {
                message: format!("Failed to acquire runtime read lock: {e}"),
            }
            .build()
        })
    }

    /// Acquire write lock on runtime selections.
    fn write_runtime(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, RoleSelections>, ConfigError> {
        self.runtime_selections.write().map_err(|e| {
            InternalSnafu {
                message: format!("Failed to acquire runtime write lock: {e}"),
            }
            .build()
        })
    }

    /// Validate that a provider exists (in config or built-in).
    fn validate_provider(&self, provider: &str) -> Result<(), ConfigError> {
        let resolver = self.read_resolver()?;

        if !resolver.has_provider(provider) {
            if builtin::get_provider_defaults(provider).is_none() {
                return NotFoundSnafu {
                    kind: NotFoundKind::Provider,
                    name: provider.to_string(),
                }
                .fail();
            }
        }

        Ok(())
    }
}

/// Suggest default models based on provider type.
fn suggest_models_for_provider(provider_type: ProviderType) -> Vec<&'static str> {
    match provider_type {
        ProviderType::Openai => vec!["gpt-5", "gpt-5.2"],
        ProviderType::Anthropic => vec!["claude-sonnet-4", "claude-opus-4"],
        ProviderType::Gemini => vec!["gemini-3-pro", "gemini-3-flash"],
        ProviderType::Volcengine => vec!["deepseek-r1", "deepseek-chat"],
        ProviderType::Zai => vec!["glm-4-plus", "glm-4-flash"],
        ProviderType::OpenaiCompat => vec!["deepseek-chat", "qwen-plus"],
    }
}

impl Clone for ConfigManager {
    fn clone(&self) -> Self {
        Self {
            config_path: self.config_path.clone(),
            loader: ConfigLoader::from_path(&self.config_path),
            resolver: RwLock::new(
                self.resolver
                    .read()
                    .expect("resolver lock poisoned")
                    .clone(),
            ),
            config: RwLock::new(self.config.read().expect("config lock poisoned").clone()),
            runtime_selections: RwLock::new(
                self.runtime_selections
                    .read()
                    .expect("runtime lock poisoned")
                    .clone(),
            ),
        }
    }
}

#[cfg(test)]
#[path = "manager.test.rs"]
mod tests;
