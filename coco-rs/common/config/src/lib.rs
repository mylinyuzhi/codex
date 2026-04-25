//! Layered configuration: settings, model selection, providers, effort, fast mode.
//!
//! Depends on coco-types for shared enums. Uses anyhow for error handling.

pub mod analytics;
pub mod constants;
pub mod effort;
pub mod env;
pub mod fast_mode;
pub mod global_config;
pub mod model;
pub mod overrides;
pub mod provider;
pub mod runtime;
pub mod sections;
pub mod settings;
pub mod system_reminder;
pub mod telemetry;

// Re-export key types for convenience
pub use analytics::AnalyticsPipeline;
pub use analytics::AnalyticsSink;
pub use analytics::EventProperties;
pub use analytics::MetadataValue;
pub use analytics::SessionAnalytics;
pub use env::EnvKey;
pub use env::EnvOnlyConfig;
pub use env::EnvSnapshot;
pub use fast_mode::CooldownReason;
pub use fast_mode::FastModeState;
pub use global_config::GlobalConfig;
pub use model::FallbackRecoveryPolicy;
pub use model::ModelInfo;
pub use model::ModelRoles;
pub use model::ModelSelection;
pub use model::ModelSelectionSettings;
pub use model::RoleSlots;
pub use model::aliases::ModelAlias;
pub use overrides::RuntimeOverrides;
pub use provider::ProviderConfig;
pub use provider::ProviderInfo;
pub use runtime::RuntimeConfig;
pub use runtime::RuntimeConfigBuilder;
pub use sections::ApiConfig;
pub use sections::ApiRetryConfig;
pub use sections::BashConfig;
pub use sections::LoopConfig;
pub use sections::McpRuntimeConfig;
pub use sections::MemoryConfig;
pub use sections::PathConfig;
pub use sections::SandboxConfig;
pub use sections::ShellConfig;
pub use sections::ToolConfig;
pub use sections::WebFetchConfig;
pub use sections::WebSearchConfig;
pub use sections::WebSearchProvider;
pub use settings::PlanModeSettings;
pub use settings::PlanModeWorkflow;
pub use settings::PlanPhase4Variant;
pub use settings::SessionSettings;
pub use settings::Settings;
pub use settings::SettingsWithSource;
pub use settings::policy::load_policy_settings;
pub use settings::source::SettingSource;
pub use settings::watcher::SettingsWatcher;
pub use system_reminder::AttachmentSettings as SystemReminderAttachmentSettings;
pub use system_reminder::DEFAULT_TIMEOUT_MS as SYSTEM_REMINDER_DEFAULT_TIMEOUT_MS;
pub use system_reminder::SystemReminderConfig;
