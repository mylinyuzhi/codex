//! Multi-provider configuration management.
//!
//! This module provides a layered configuration system for managing multiple
//! LLM providers, models, and profiles. Configuration is stored in JSON files
//! in the `~/.cocode` directory by default.
//!
//! # Configuration Files
//!
//! - `models.json`: Provider-independent model metadata
//! - `providers.json`: Provider access configuration
//! - `profiles.json`: Named configuration bundles for quick switching
//! - `active.json`: Runtime state (managed by SDK)
//!
//! # Configuration Resolution
//!
//! Values are resolved with the following precedence (highest to lowest):
//! 1. Runtime overrides (API calls, `/model` command)
//! 2. Environment variables (for secrets)
//! 3. Provider-specific model override
//! 4. User model config (`models.json`)
//! 5. Built-in defaults (compiled into binary)
//!
//! # Example
//!
//! ```no_run
//! use cocode_config::ConfigManager;
//! use cocode_config::error::ConfigError;
//!
//! # fn example() -> Result<(), ConfigError> {
//! // Load from default path (~/.cocode)
//! let manager = ConfigManager::from_default()?;
//!
//! // Get current provider/model
//! let (provider, model) = manager.current();
//! println!("Using: {provider}/{model}");
//!
//! // Switch to a different provider/model
//! manager.switch("anthropic", "claude-sonnet-4-20250514")?;
//!
//! // Or switch to a named profile
//! manager.switch_profile("coding")?;
//!
//! // Get resolved model info
//! let info = manager.resolve_model_info("anthropic", "claude-sonnet-4-20250514")?;
//! println!("Context window: {}", info.context_window);
//! # Ok(())
//! # }
//! ```

pub mod builtin;
pub mod error;
pub mod loader;
pub mod manager;
pub mod resolver;
pub mod types;

// Re-export protocol types
pub use cocode_protocol::Capability;
pub use cocode_protocol::ConfigShellToolType;
pub use cocode_protocol::ModelInfo;
pub use cocode_protocol::ReasoningEffort;
pub use cocode_protocol::TruncationMode;
pub use cocode_protocol::TruncationPolicyConfig;

// Re-export main config types
pub use loader::ConfigLoader;
pub use loader::DEFAULT_CONFIG_DIR;
pub use loader::LoadedConfig;
pub use manager::ConfigManager;
pub use resolver::ConfigResolver;
pub use types::ActiveState;
pub use types::ModelSummary;
pub use types::ModelsFile;
pub use types::ProfileConfig;
pub use types::ProfilesFile;
pub use types::ProviderConfig;
pub use types::ProviderModelConfig;
pub use types::ProviderSummary;
pub use types::ProviderType;
pub use types::ProvidersFile;
pub use types::ResolvedModelInfo;
pub use types::ResolvedProviderConfig;
pub use types::SessionConfigJson;
