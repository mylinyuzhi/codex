//! Complete runtime configuration snapshot for building an agent.
//!
//! This module provides the `Config` struct which is a complete runtime snapshot
//! containing all resolved configuration needed to build and run an agent.
//!
//! ## Relationship with other types
//!
//! - `AppConfig`: JSON file format, supports profile switching
//! - `ConfigManager`: Loading, caching, runtime switching
//! - `Config`: Complete runtime snapshot with resolved values
//!
//! ## Usage
//!
//! ```no_run
//! use cocode_config::{ConfigManager, ConfigOverrides};
//!
//! # fn example() -> Result<(), cocode_config::error::ConfigError> {
//! let manager = ConfigManager::from_default()?;
//! let config = manager.build_config(ConfigOverrides::default())?;
//!
//! // Access main model
//! if let Some(main) = config.main_model_info() {
//!     println!("Main model: {} ({:?})", main.display_name_or_slug(), main.context_window);
//! }
//!
//! // Access role-specific model
//! use cocode_protocol::model::ModelRole;
//! if let Some(fast) = config.model_for_role(ModelRole::Fast) {
//!     println!("Fast model: {}", fast.display_name_or_slug());
//! }
//! # Ok(())
//! # }
//! ```

use crate::json_config::LoggingConfig;
use crate::json_config::PermissionsConfig;
use cocode_protocol::AttachmentConfig;
use cocode_protocol::CompactConfig;
use cocode_protocol::Features;
use cocode_protocol::ModelInfo;
use cocode_protocol::PathConfig;
use cocode_protocol::PlanModeConfig;
use cocode_protocol::ProviderInfo;
use cocode_protocol::ProviderType;
use cocode_protocol::RoleSelection;
use cocode_protocol::SandboxMode;
use cocode_protocol::ToolConfig;
use cocode_protocol::WebFetchConfig;
use cocode_protocol::WebSearchConfig;
use cocode_protocol::model::ModelRole;
use cocode_protocol::model::ModelRoles;
use cocode_protocol::model::ModelSpec;
use std::collections::HashMap;
use std::path::PathBuf;

/// Complete runtime configuration snapshot for building an agent.
///
/// This struct contains all the resolved configuration needed to build
/// and run an agent. It is created from `ConfigManager::build_config()`.
///
/// ## ModelRoles support
///
/// Supports all 6 roles (Main, Fast, Vision, Review, Plan, Explore).
/// Use `model_for_role()` to get resolved info for a specific role.
///
/// ## Fields
///
/// The Config struct is organized into logical sections:
/// - **Model & Provider**: Complete ModelRoles support and resolved providers
/// - **Paths**: Working directory and cocode home
/// - **Instructions**: User instructions from AGENTS.md
/// - **Features**: Centralized feature flags
/// - **Session**: Logging and profile settings
/// - **Sandbox**: Filesystem access control
#[derive(Debug, Clone)]
pub struct Config {
    // ============================================================
    // 1. Model & Provider
    // ============================================================
    /// Role-based model configuration (all 6 roles).
    pub models: ModelRoles,

    /// All available providers (resolved with API keys).
    pub providers: HashMap<String, ProviderInfo>,

    /// Cached resolved model info for each configured role.
    pub(crate) resolved_models: HashMap<ModelRole, ModelInfo>,

    // ============================================================
    // 2. Paths
    // ============================================================
    /// Current working directory for the session.
    pub cwd: PathBuf,

    /// Cocode home directory (default: ~/.cocode).
    pub cocode_home: PathBuf,

    // ============================================================
    // 3. Instructions
    // ============================================================
    /// User instructions from AGENTS.md.
    pub user_instructions: Option<String>,

    // ============================================================
    // 4. Features
    // ============================================================
    /// Centralized feature flags (resolved).
    pub features: Features,

    // ============================================================
    // 5. Session
    // ============================================================
    /// Logging configuration.
    pub logging: Option<LoggingConfig>,

    /// Active profile name.
    pub active_profile: Option<String>,

    /// Session is ephemeral (not persisted).
    pub ephemeral: bool,

    // ============================================================
    // 6. Sandbox
    // ============================================================
    /// Sandbox mode for filesystem access.
    pub sandbox_mode: SandboxMode,

    /// Writable roots for sandbox (when WorkspaceWrite).
    pub writable_roots: Vec<PathBuf>,

    // ============================================================
    // 7. Tool Execution
    // ============================================================
    /// Tool execution configuration.
    pub tool_config: ToolConfig,

    // ============================================================
    // 8. Compaction
    // ============================================================
    /// Compaction and session memory configuration.
    pub compact_config: CompactConfig,

    // ============================================================
    // 9. Plan Mode
    // ============================================================
    /// Plan mode configuration.
    pub plan_config: PlanModeConfig,

    // ============================================================
    // 10. Attachments
    // ============================================================
    /// Attachment configuration.
    pub attachment_config: AttachmentConfig,

    // ============================================================
    // 11. Extended Paths
    // ============================================================
    /// Extended path configuration.
    pub path_config: PathConfig,

    // ============================================================
    // 12. Web Search
    // ============================================================
    /// Web search configuration (provider, api_key, max_results).
    pub web_search_config: WebSearchConfig,

    // ============================================================
    // 13. Web Fetch
    // ============================================================
    /// Web fetch configuration (timeout, max_content_length, user_agent).
    pub web_fetch_config: WebFetchConfig,

    // ============================================================
    // 14. Permissions
    // ============================================================
    /// Permission rules from config (allow/deny/ask patterns).
    pub permissions: Option<PermissionsConfig>,

    // ============================================================
    // 15. Hooks
    // ============================================================
    /// Hook definitions from config.json.
    pub hooks: Vec<crate::json_config::HookConfig>,

    // ============================================================
    // 16. OpenTelemetry
    // ============================================================
    /// OpenTelemetry settings (resolved from JSON config + env vars).
    pub otel: Option<cocode_otel::config::OtelSettings>,

    // ============================================================
    // 17. Output Style
    // ============================================================
    /// Active output style name (e.g., "explanatory", "learning", or custom).
    pub output_style: Option<String>,
}

impl Config {
    /// Get resolved model info for a specific role.
    ///
    /// Falls back to Main role if the specific role is not configured.
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
    /// // Get fast model (falls back to main if not configured)
    /// if let Some(fast) = config.model_for_role(ModelRole::Fast) {
    ///     println!("Using model: {}", fast.display_name_or_slug());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn model_for_role(&self, role: ModelRole) -> Option<&ModelInfo> {
        self.resolved_models
            .get(&role)
            .or_else(|| self.resolved_models.get(&ModelRole::Main))
    }

    /// Get provider info for a specific role.
    ///
    /// Looks up the provider based on the model spec for the given role.
    pub fn provider_for_role(&self, role: ModelRole) -> Option<&ProviderInfo> {
        let spec = self.models.get(role)?;
        self.providers.get(&spec.provider)
    }

    /// Get the main model spec.
    pub fn main_model(&self) -> Option<&ModelSpec> {
        self.models.main()
    }

    /// Get resolved info for main model.
    pub fn main_model_info(&self) -> Option<&ModelInfo> {
        self.model_for_role(ModelRole::Main)
    }

    /// Get model spec for a role (with fallback to main).
    pub fn model_spec_for_role(&self, role: ModelRole) -> Option<&ModelSpec> {
        self.models.get(role)
    }

    /// Get provider info by name.
    pub fn provider(&self, name: &str) -> Option<&ProviderInfo> {
        self.providers.get(name)
    }

    /// Get all configured role-model pairs.
    pub fn configured_roles(&self) -> Vec<(ModelRole, &ModelInfo)> {
        self.resolved_models
            .iter()
            .map(|(role, info)| (*role, info))
            .collect()
    }

    /// Check if a specific feature is enabled.
    pub fn is_feature_enabled(&self, feature: cocode_protocol::Feature) -> bool {
        self.features.enabled(feature)
    }

    /// Check if sandbox allows write operations.
    pub fn allows_write(&self) -> bool {
        self.sandbox_mode.allows_write()
    }

    /// Get complete ProviderModel (ModelInfo + alias) for a provider/model.
    ///
    /// This is a more efficient alternative to calling both `resolve_model_info()`
    /// and `resolve_model_alias()` separately, as it performs a single lookup.
    pub fn resolve_provider_model(
        &self,
        provider: &str,
        model: &str,
    ) -> Option<&cocode_protocol::ProviderModel> {
        self.providers
            .get(provider)
            .and_then(|p| p.models.get(model))
    }

    /// Get `ModelInfo` for a specific provider/model (for ModelHub model creation).
    pub fn resolve_model_info(&self, provider: &str, model: &str) -> Option<&ModelInfo> {
        self.providers
            .get(provider)
            .and_then(|p| p.models.get(model))
            .map(|m| &m.info)
    }

    /// Get API model name (alias) for a provider/model (for ModelHub model creation).
    ///
    /// Returns the alias if set and non-empty, otherwise returns the slug.
    pub fn resolve_model_alias<'a>(&'a self, provider: &str, model: &'a str) -> &'a str {
        self.providers
            .get(provider)
            .and_then(|p| p.models.get(model))
            .and_then(|m| m.model_alias.as_deref())
            .unwrap_or(model)
    }

    /// Get `ProviderType` by name.
    pub fn provider_type(&self, name: &str) -> Option<ProviderType> {
        self.providers.get(name).map(|p| p.provider_type)
    }

    /// Build `RoleSelection`s for all configured models across all providers.
    ///
    /// Used by the TUI model picker.
    pub fn all_model_selections(&self) -> Vec<RoleSelection> {
        let mut selections = Vec::new();
        for (provider_name, provider_info) in &self.providers {
            for (slug, provider_model) in &provider_info.models {
                let info = &provider_model.info;
                let spec = ModelSpec::with_type(provider_name, provider_info.provider_type, slug)
                    .with_display_name(info.display_name_or_slug());

                let mut selection = match info.default_thinking_level {
                    Some(ref level) => RoleSelection::with_thinking(spec, level.clone()),
                    None => RoleSelection::new(spec),
                };
                selection.supported_thinking_levels = info.supported_thinking_levels.clone();
                selections.push(selection);
            }
        }
        selections
    }

    /// Check if a path is writable under current sandbox mode.
    pub fn is_path_writable(&self, path: &std::path::Path) -> bool {
        match self.sandbox_mode {
            SandboxMode::ReadOnly => false,
            SandboxMode::FullAccess => true,
            SandboxMode::WorkspaceWrite => {
                // Check if path is under any writable root
                self.writable_roots
                    .iter()
                    .any(|root| path.starts_with(root))
            }
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            models: ModelRoles::default(),
            providers: HashMap::new(),
            resolved_models: HashMap::new(),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            cocode_home: crate::loader::default_config_dir(),
            user_instructions: None,
            features: Features::with_defaults(),
            logging: None,
            active_profile: None,
            ephemeral: false,
            sandbox_mode: SandboxMode::default(),
            writable_roots: Vec::new(),
            tool_config: ToolConfig::default(),
            compact_config: CompactConfig::default(),
            plan_config: PlanModeConfig::default(),
            attachment_config: AttachmentConfig::default(),
            path_config: PathConfig::default(),
            web_search_config: WebSearchConfig::default(),
            web_fetch_config: WebFetchConfig::default(),
            permissions: None,
            hooks: Vec::new(),
            otel: None,
            output_style: None,
        }
    }
}

/// Configuration overrides for building a Config.
///
/// These overrides are applied on top of the resolved configuration
/// from ConfigManager.
#[derive(Debug, Clone, Default)]
pub struct ConfigOverrides {
    /// Override for specific roles.
    pub models: Option<ModelRoles>,

    /// Override working directory.
    pub cwd: Option<PathBuf>,

    /// Override sandbox mode.
    pub sandbox_mode: Option<SandboxMode>,

    /// Override ephemeral flag.
    pub ephemeral: Option<bool>,

    /// Feature overrides (key -> enabled).
    pub features: HashMap<String, bool>,

    /// Writable roots for sandbox.
    pub writable_roots: Option<Vec<PathBuf>>,

    /// Override user instructions.
    pub user_instructions: Option<String>,

    /// Override tool configuration.
    pub tool_config: Option<ToolConfig>,

    /// Override compaction configuration.
    pub compact_config: Option<CompactConfig>,

    /// Override plan mode configuration.
    pub plan_config: Option<PlanModeConfig>,

    /// Override attachment configuration.
    pub attachment_config: Option<AttachmentConfig>,

    /// Override path configuration.
    pub path_config: Option<PathConfig>,
}

impl ConfigOverrides {
    /// Create new empty overrides.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set model overrides.
    pub fn with_models(mut self, models: ModelRoles) -> Self {
        self.models = Some(models);
        self
    }

    /// Set working directory.
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Set sandbox mode.
    pub fn with_sandbox_mode(mut self, mode: SandboxMode) -> Self {
        self.sandbox_mode = Some(mode);
        self
    }

    /// Set ephemeral flag.
    pub fn with_ephemeral(mut self, ephemeral: bool) -> Self {
        self.ephemeral = Some(ephemeral);
        self
    }

    /// Add a feature override.
    pub fn with_feature(mut self, key: impl Into<String>, enabled: bool) -> Self {
        self.features.insert(key.into(), enabled);
        self
    }

    /// Set writable roots.
    pub fn with_writable_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.writable_roots = Some(roots);
        self
    }

    /// Set user instructions.
    pub fn with_user_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.user_instructions = Some(instructions.into());
        self
    }

    /// Set tool configuration.
    pub fn with_tool_config(mut self, config: ToolConfig) -> Self {
        self.tool_config = Some(config);
        self
    }

    /// Set compaction configuration.
    pub fn with_compact_config(mut self, config: CompactConfig) -> Self {
        self.compact_config = Some(config);
        self
    }

    /// Set plan mode configuration.
    pub fn with_plan_config(mut self, config: PlanModeConfig) -> Self {
        self.plan_config = Some(config);
        self
    }

    /// Set attachment configuration.
    pub fn with_attachment_config(mut self, config: AttachmentConfig) -> Self {
        self.attachment_config = Some(config);
        self
    }

    /// Set path configuration.
    pub fn with_path_config(mut self, config: PathConfig) -> Self {
        self.path_config = Some(config);
        self
    }
}

#[cfg(test)]
#[path = "config.test.rs"]
mod tests;
