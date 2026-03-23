//! Configuration manager with caching and runtime switching.
//!
//! `ConfigManager` is the main entry point for configuration management.
//! It handles loading, caching, and runtime switching of providers and models.

use crate::builtin;
use crate::config::Config;
use crate::config::ConfigOverrides;
use crate::constraint::validate_model_info_fields;
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
use cocode_protocol::ProviderApi;
use cocode_protocol::ProviderInfo;
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
use std::sync::RwLockReadGuard;
use std::sync::RwLockWriteGuard;
use tracing::debug;
use tracing::info;
use tracing::warn;

/// Type alias for the result of `build_model_cache()`: (models by role, providers).
type ModelCacheResult = (HashMap<ModelRole, ModelInfo>, HashMap<String, ProviderInfo>);

/// Acquire a read lock, converting poison errors to `ConfigError`.
fn acquire_read<'a, T>(
    lock: &'a RwLock<T>,
    name: &str,
) -> Result<RwLockReadGuard<'a, T>, ConfigError> {
    lock.read().map_err(|e| {
        InternalSnafu {
            message: format!("Failed to acquire {name} read lock: {e}"),
        }
        .build()
    })
}

/// Acquire a write lock, converting poison errors to `ConfigError`.
fn acquire_write<'a, T>(
    lock: &'a RwLock<T>,
    name: &str,
) -> Result<RwLockWriteGuard<'a, T>, ConfigError> {
    lock.write().map_err(|e| {
        InternalSnafu {
            message: format!("Failed to acquire {name} write lock: {e}"),
        }
        .build()
    })
}

/// Clone an `RwLock`'s contents, recovering from poison.
fn clone_rwlock_recovering<T: Clone>(lock: &RwLock<T>, name: &str) -> RwLock<T> {
    RwLock::new(
        lock.read()
            .unwrap_or_else(|e| {
                tracing::warn!("{name} lock poisoned, recovering");
                e.into_inner()
            })
            .clone(),
    )
}

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
/// // Get current main model
/// use cocode_protocol::model::ModelRole;
/// if let Some(spec) = manager.current_spec_for_role(ModelRole::Main) {
///     println!("Current: {}/{}", spec.provider, spec.slug);
/// }
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
    /// the `ProviderApi` — it skips API key validation and model resolution.
    pub fn provider_api(&self, provider: &str) -> Result<ProviderApi, ConfigError> {
        let resolver = self.read_resolver()?;
        resolver.provider_api(provider)
    }

    /// Resolve the API model name for a given slug (O(1) lookup, no resolution).
    ///
    /// Returns the api_model_name if set and non-empty, otherwise returns the slug.
    /// The result is an owned `String` because the resolver is behind a `RwLock`.
    pub fn resolve_api_model_name(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<String, ConfigError> {
        let resolver = self.read_resolver()?;
        Ok(resolver.resolve_api_model_name(provider, model).to_string())
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

    /// Get the current active provider and model for a specific role as a `ModelSpec`.
    ///
    /// Resolution order (highest to lowest precedence):
    /// 1. Runtime overrides (per-role, no main fallback)
    /// 2. JSON config with profile resolution (per-role, no main fallback)
    ///
    /// Returns `None` if the specific role isn't configured.
    /// Role->main fallback belongs at the consumer level (`RoleSelections::get_or_main()`).
    pub fn current_spec_for_role(&self, role: ModelRole) -> Option<ModelSpec> {
        // 1. Check runtime overrides first (per-role, no main fallback)
        if let Ok(runtime) = self.read_runtime()
            && let Some(selection) = runtime.get(role)
        {
            return Some(selection.model.clone());
        }

        // 2. Check JSON config (with profile resolution, per-role, no main fallback)
        if let Ok(config) = self.read_config() {
            let resolved = config.resolve();
            if let Some(spec) = resolved.models.get_direct(role) {
                let mut spec = spec.clone();
                if let Ok(info) = self.resolve_model_info(&spec.provider, &spec.slug) {
                    spec.enrich_from_model_info(&info);
                }
                return Some(spec);
            }
        }

        None
    }

    /// Switch to a specific provider and model using a typed `ModelSpec`.
    ///
    /// This updates the runtime overrides (in-memory only).
    /// To persist, edit `config.toml` directly.
    pub fn switch_spec(&self, spec: &ModelSpec) -> Result<(), ConfigError> {
        self.switch_role_spec(ModelRole::Main, spec)
    }

    /// Switch model for a specific role using a typed `ModelSpec`.
    ///
    /// This updates the runtime overrides for the specified role.
    /// To persist, edit the config file directly.
    pub fn switch_role_spec(&self, role: ModelRole, spec: &ModelSpec) -> Result<(), ConfigError> {
        // Validate the provider
        self.validate_provider(&spec.provider)?;

        // Resolve full selection with thinking levels from ModelInfo
        let selection = self
            .resolve_selection(&spec.provider, &spec.slug)
            .unwrap_or_else(|_| RoleSelection::new(spec.clone()));

        // Update runtime selections
        let mut runtime = self.write_runtime()?;
        runtime.set(role, selection);

        info!(
            provider = %spec.provider,
            model = %spec.slug,
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
            model = %spec.slug,
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
        let mut config = self.write_config()?;

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

    /// Build a `RoleSelection` for a provider/model pair, resolving provider_api
    /// and the model's default thinking level from `ModelInfo`.
    pub fn resolve_selection(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<RoleSelection, ConfigError> {
        let api = self.provider_api(provider)?;
        let spec = ModelSpec::with_type(provider, api, model);
        let model_info = self.resolve_model_info(provider, model).ok();
        Ok(Self::build_selection(spec, model_info))
    }

    /// Build a `RoleSelection` from a `ModelSpec` and optional `ModelInfo`.
    ///
    /// Populates display_name, thinking_level, and supported_thinking_levels
    /// from the model info when available.
    fn build_selection(mut spec: ModelSpec, model_info: Option<ModelInfo>) -> RoleSelection {
        let Some(info) = model_info else {
            return RoleSelection::new(spec);
        };
        spec.display_name = info.display_name_or_slug().to_string();
        let mut selection = match info.default_thinking_level.clone() {
            Some(level) => RoleSelection::with_thinking(spec, level),
            None => RoleSelection::new(spec),
        };
        selection.supported_thinking_levels = info.supported_thinking_levels;
        selection
    }

    /// Build a `RoleSelection` for the current main model.
    ///
    /// Returns `None` if main model is not configured.
    pub fn current_main_selection(&self) -> Option<RoleSelection> {
        let spec = self.current_spec_for_role(ModelRole::Main)?;
        Some(
            self.resolve_selection(&spec.provider, &spec.slug)
                .unwrap_or_else(|_| RoleSelection::new(spec)),
        )
    }

    /// Build `RoleSelections` for ALL configured roles.
    ///
    /// Populates each role with correct provider_api, display_name,
    /// thinking_level, and supported_thinking_levels from ModelInfo.
    /// Non-configured roles remain None (fallback to main via `get_or_main()`).
    pub fn build_all_selections(&self) -> RoleSelections {
        let mut selections = RoleSelections::default();

        // Start from JSON config
        let models = if let Ok(config) = self.read_config() {
            config.resolve().models
        } else {
            return selections;
        };

        // Apply runtime overrides
        let runtime = self.read_runtime().ok();

        for &role in ModelRole::all() {
            // Runtime override takes precedence
            if let Some(ref rt) = runtime
                && let Some(sel) = rt.get(role)
            {
                selections.set(role, sel.clone());
                continue;
            }

            // Then JSON config (no main fallback)
            if let Some(spec) = models.get_direct(role) {
                let selection = self
                    .resolve_selection(&spec.provider, &spec.slug)
                    .unwrap_or_else(|_| RoleSelection::new(spec.clone()));
                selections.set(role, selection);
            }
        }

        selections
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
            let Ok(provider_api) = resolver.provider_api(provider_name) else {
                continue;
            };
            for slug in resolver.list_models(provider_name) {
                let model_info = resolver.resolve_model_info(provider_name, slug).ok();
                let spec = ModelSpec::with_type(provider_name, provider_api, slug);
                selections.push(Self::build_selection(spec, model_info));
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

        *self.write_resolver()? = new_resolver;
        *self.write_config()? = loaded.config;

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
            if !summaries.iter().any(|s| s.name == name)
                && let Some(config) = builtin::get_provider_defaults(name)
            {
                summaries.push(ProviderSummary::from_builtin(name, &config));
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
        if summaries.is_empty()
            && let Some(provider_config) = resolver.get_provider_config(provider)
        {
            let suggested = suggest_models_for_provider(provider_config.api);
            for model_id in suggested {
                if let Some(info) = builtin::get_model_defaults(model_id) {
                    summaries.push(ModelSummary::from_model_info(model_id, &info));
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

        // Validate resolved model info fields (warn on constraint violations)
        for (role, info) in &resolved_models {
            for err in validate_model_info_fields(info) {
                warn!(
                    role = %role,
                    model = %info.slug,
                    "{err}"
                );
            }
        }

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
        let auto_memory_config = resolved.auto_memory.clone().unwrap_or_default();
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
            auto_memory_config,
            path_config,
            web_search_config,
            web_fetch_config,
            permissions: resolved.permissions.clone(),
            hooks: resolved.hooks.clone(),
            disable_all_hooks: resolved.disable_all_hooks,
            allow_managed_hooks_only: resolved.allow_managed_hooks_only,
            otel,
            output_style: resolved.output_style,
            enabled_plugins: resolved.enabled_plugins,
            extra_known_marketplaces: resolved.extra_known_marketplaces,
        })
    }

    // Private helper methods for config building

    /// Build resolved models and providers from configured model roles.
    ///
    /// Returns (resolved_models_by_role, providers).
    fn build_model_cache(
        &self,
        models: &cocode_protocol::ModelRoles,
    ) -> Result<ModelCacheResult, ConfigError> {
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

    fn read_resolver(&self) -> Result<RwLockReadGuard<'_, ConfigResolver>, ConfigError> {
        acquire_read(&self.resolver, "resolver")
    }

    fn read_config(&self) -> Result<RwLockReadGuard<'_, AppConfig>, ConfigError> {
        acquire_read(&self.config, "config")
    }

    fn read_runtime(&self) -> Result<RwLockReadGuard<'_, RoleSelections>, ConfigError> {
        acquire_read(&self.runtime_selections, "runtime")
    }

    fn write_resolver(&self) -> Result<RwLockWriteGuard<'_, ConfigResolver>, ConfigError> {
        acquire_write(&self.resolver, "resolver")
    }

    fn write_config(&self) -> Result<RwLockWriteGuard<'_, AppConfig>, ConfigError> {
        acquire_write(&self.config, "config")
    }

    fn write_runtime(&self) -> Result<RwLockWriteGuard<'_, RoleSelections>, ConfigError> {
        acquire_write(&self.runtime_selections, "runtime")
    }

    /// Validate that a provider exists (in config or built-in).
    fn validate_provider(&self, provider: &str) -> Result<(), ConfigError> {
        let resolver = self.read_resolver()?;

        if !resolver.has_provider(provider) && builtin::get_provider_defaults(provider).is_none() {
            return NotFoundSnafu {
                kind: NotFoundKind::Provider,
                name: provider.to_string(),
            }
            .fail();
        }

        Ok(())
    }
}

/// Suggest default models based on provider type.
fn suggest_models_for_provider(api: ProviderApi) -> Vec<&'static str> {
    match api {
        ProviderApi::Openai => vec!["gpt-5", "gpt-5.2"],
        ProviderApi::Anthropic => vec!["claude-sonnet-4", "claude-opus-4"],
        ProviderApi::Gemini => vec!["gemini-3-pro", "gemini-3-flash"],
        ProviderApi::Volcengine => vec!["deepseek-r1", "deepseek-chat"],
        ProviderApi::Zai => vec!["glm-4-plus", "glm-4-flash"],
        ProviderApi::OpenaiCompat => vec!["deepseek-chat", "qwen-plus"],
    }
}

impl Clone for ConfigManager {
    fn clone(&self) -> Self {
        Self {
            config_path: self.config_path.clone(),
            loader: ConfigLoader::from_path(&self.config_path),
            resolver: clone_rwlock_recovering(&self.resolver, "resolver"),
            config: clone_rwlock_recovering(&self.config, "config"),
            runtime_selections: clone_rwlock_recovering(
                &self.runtime_selections,
                "runtime_selections",
            ),
        }
    }
}

#[cfg(test)]
#[path = "manager.test.rs"]
mod tests;
