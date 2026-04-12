//! Layered configuration: settings, model selection, providers, effort, fast mode.
//!
//! Depends on coco-types for shared enums. Uses anyhow for error handling.

pub mod analytics;
pub mod bootstrap;
pub mod constants;
pub mod effort;
pub mod env;
pub mod fast_mode;
pub mod global_config;
pub mod migrations;
pub mod model;
pub mod overrides;
pub mod provider;
pub mod resolved;
pub mod settings;
pub mod telemetry;

// Re-export key types for convenience
pub use analytics::AnalyticsPipeline;
pub use analytics::AnalyticsSink;
pub use analytics::EventProperties;
pub use analytics::MetadataValue;
pub use analytics::SessionAnalytics;
pub use bootstrap::BootstrapConfig;
pub use bootstrap::SessionState;
pub use env::EnvOnlyConfig;
pub use fast_mode::CooldownReason;
pub use fast_mode::FastModeState;
pub use global_config::GlobalConfig;
pub use model::ModelInfo;
pub use model::ModelRoles;
pub use model::aliases::ModelAlias;
pub use model::get_agent_model;
pub use model::get_main_loop_model;
pub use overrides::RuntimeOverrides;
pub use provider::ProviderConfig;
pub use provider::ProviderInfo;
pub use resolved::ResolvedConfig;
pub use settings::Settings;
pub use settings::SettingsWithSource;
pub use settings::policy::load_policy_settings;
pub use settings::source::SettingSource;
pub use settings::watcher::SettingsWatcher;
