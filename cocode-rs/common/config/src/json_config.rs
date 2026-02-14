//! JSON configuration types for config.json.
//!
//! This module defines the file format types for `~/.cocode/config.json`.
//! These types represent the JSON structure and are separate from the runtime
//! feature types in `cocode_protocol::features`.
//!
//! # Profile System
//!
//! Profiles allow quick switching between different model/provider configurations.
//! Profiles are defined inline in `config.json` and can override top-level settings.
//!
//! ## Resolution Order
//!
//! 1. Profile field (if profile is selected)
//! 2. Top-level field
//! 3. Built-in default
//!
//! ## Example
//!
//! ```json
//! {
//!   "models": {
//!     "main": "anthropic/claude-opus-4",
//!     "fast": "anthropic/claude-haiku",
//!     "vision": "openai/gpt-4o"
//!   },
//!   "logging": {
//!     "level": "info"
//!   },
//!   "features": {
//!     "web_fetch": true
//!   },
//!   "profile": "fast",
//!   "profiles": {
//!     "openai": {
//!       "models": {
//!         "main": "openai/gpt-5",
//!         "fast": "openai/gpt-5-mini"
//!       }
//!     },
//!     "debug": {
//!       "logging": {
//!         "level": "debug",
//!         "location": true
//!       }
//!     }
//!   }
//! }
//! ```

use cocode_protocol::AttachmentConfig;
use cocode_protocol::CompactConfig;
use cocode_protocol::Features;
use cocode_protocol::PathConfig;
use cocode_protocol::PlanModeConfig;
use cocode_protocol::ToolConfig;
use cocode_protocol::WebFetchConfig;
use cocode_protocol::WebSearchConfig;
use cocode_protocol::model::ModelRoles;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::HashMap;

/// Permission rules configuration section.
///
/// Defines allow/deny/ask rules for tool execution.
/// Rules follow the pattern: tool name optionally followed by a command
/// pattern in parentheses, e.g. `"Bash(git *)"`, `"Read"`, `"Edit"`.
///
/// # Example
///
/// ```json
/// {
///   "permissions": {
///     "allow": ["Read", "Glob", "Bash(git *)", "Bash(npm *)"],
///     "deny": ["Bash(rm -rf *)"],
///     "ask": ["Bash(sudo *)"]
///   }
/// }
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct PermissionsConfig {
    /// Tool patterns that are always allowed without prompting.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Tool patterns that are always denied.
    #[serde(default)]
    pub deny: Vec<String>,
    /// Tool patterns that require user approval each time.
    #[serde(default)]
    pub ask: Vec<String>,
}

/// Profile configuration that can override top-level settings.
///
/// All fields are optional - only set fields will override top-level config.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct ConfigProfile {
    /// Role-based model configuration.
    #[serde(default)]
    pub models: Option<ModelRoles>,

    /// Override features.
    #[serde(default)]
    pub features: Option<FeaturesConfig>,

    /// Override logging.
    #[serde(default)]
    pub logging: Option<LoggingConfig>,
}

/// Application configuration file (~/.cocode/config.json).
///
/// # Example
///
/// ```json
/// {
///   "models": {
///     "main": "anthropic/claude-opus-4",
///     "fast": "anthropic/claude-haiku",
///     "vision": "openai/gpt-4o"
///   },
///   "logging": {
///     "level": "debug",
///     "location": true,
///     "target": false
///   },
///   "features": {
///     "web_fetch": true,
///     "web_fetch": true
///   },
///   "profile": "fast",
///   "profiles": {
///     "fast": {
///       "models": {
///         "fast": "openai/gpt-5-mini"
///       }
///     }
///   }
/// }
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct AppConfig {
    /// Role-based model configuration.
    #[serde(default)]
    pub models: Option<ModelRoles>,

    /// Profile name to use (selects from `profiles` table).
    #[serde(default)]
    pub profile: Option<String>,

    /// Logging configuration.
    #[serde(default)]
    pub logging: Option<LoggingConfig>,

    /// Feature toggles.
    #[serde(default)]
    pub features: Option<FeaturesConfig>,

    /// Profile definitions for quick switching.
    #[serde(default)]
    pub profiles: HashMap<String, ConfigProfile>,

    /// Tool execution configuration.
    #[serde(default)]
    pub tool: Option<ToolConfig>,

    /// Compaction configuration.
    #[serde(default)]
    pub compact: Option<CompactConfig>,

    /// Plan mode configuration.
    #[serde(default)]
    pub plan: Option<PlanModeConfig>,

    /// Attachment configuration.
    #[serde(default)]
    pub attachment: Option<AttachmentConfig>,

    /// Extended path configuration.
    #[serde(default)]
    pub paths: Option<PathConfig>,

    /// Preferred language for responses (e.g., "en", "zh", "ja").
    /// When set, the agent will respond in this language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_preference: Option<String>,

    /// Permission rules for tool execution.
    #[serde(default)]
    pub permissions: Option<PermissionsConfig>,

    /// Web search configuration (provider, api_key, max_results).
    #[serde(default)]
    pub web_search: Option<WebSearchConfig>,

    /// Web fetch configuration (timeout, max_content_length, user_agent).
    #[serde(default)]
    pub web_fetch: Option<WebFetchConfig>,

    /// Hook definitions for event interception.
    ///
    /// # Example
    ///
    /// ```json
    /// {
    ///   "hooks": [
    ///     {
    ///       "event": "pre_tool_use",
    ///       "matcher": "Bash",
    ///       "hooks": [
    ///         { "type": "command", "command": "my-lint-check" }
    ///       ]
    ///     }
    ///   ]
    /// }
    /// ```
    #[serde(default)]
    pub hooks: Vec<HookConfig>,

    /// OpenTelemetry configuration.
    #[serde(default)]
    pub otel: Option<OtelJsonConfig>,

    /// Output style name to activate (e.g., "explanatory", "learning", or a custom style).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "outputStyle"
    )]
    pub output_style: Option<String>,
}

/// A single hook configuration entry in config.json.
///
/// Matches Claude Code v2.1.7 hook format.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct HookConfig {
    /// Event type: "pre_tool_use", "post_tool_use", "session_start", etc.
    pub event: String,

    /// Tool name pattern to match (exact match or pipe-separated "A|B").
    /// If empty or absent, matches all tools for tool events.
    #[serde(default)]
    pub matcher: Option<String>,

    /// List of hook handlers to execute for this event.
    #[serde(default)]
    pub hooks: Vec<HookHandlerConfig>,
}

/// A single hook handler in config.json.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookHandlerConfig {
    /// Execute a shell command.
    Command {
        /// Command to execute.
        command: String,
        /// Optional arguments.
        #[serde(default)]
        args: Vec<String>,
        /// Timeout in seconds (default: 30).
        #[serde(default = "default_hook_timeout")]
        timeout_secs: i32,
    },
}

fn default_hook_timeout() -> i32 {
    30
}

/// Resolved configuration with profile applied.
///
/// This is the effective configuration after merging profile overrides
/// with top-level settings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedAppConfig {
    /// Effective role-based models.
    pub models: ModelRoles,
    /// Effective logging configuration.
    pub logging: Option<LoggingConfig>,
    /// Effective features.
    pub features: Features,
    /// Effective tool configuration.
    pub tool: Option<ToolConfig>,
    /// Effective compaction configuration.
    pub compact: Option<CompactConfig>,
    /// Effective plan mode configuration.
    pub plan: Option<PlanModeConfig>,
    /// Effective attachment configuration.
    pub attachment: Option<AttachmentConfig>,
    /// Effective path configuration.
    pub paths: Option<PathConfig>,
    /// Effective language preference.
    pub language_preference: Option<String>,
    /// Effective permission rules.
    pub permissions: Option<PermissionsConfig>,
    /// Effective web search configuration.
    pub web_search: Option<WebSearchConfig>,
    /// Effective web fetch configuration.
    pub web_fetch: Option<WebFetchConfig>,
    /// Effective hook definitions.
    pub hooks: Vec<HookConfig>,
    /// Effective OTel configuration.
    pub otel: Option<OtelJsonConfig>,
    /// Effective output style name.
    pub output_style: Option<String>,
}

impl AppConfig {
    /// Resolve effective config with profile applied.
    ///
    /// Priority: Profile field > Top-level field > Built-in default
    pub fn resolve(&self) -> ResolvedAppConfig {
        let profile = self
            .profile
            .as_ref()
            .and_then(|name| self.profiles.get(name));

        ResolvedAppConfig {
            models: self.resolve_models(profile),
            logging: self.resolve_logging(profile),
            features: self.resolve_features_with_profile(profile),
            tool: self.tool.clone(),
            compact: self.compact.clone(),
            plan: self.plan.clone(),
            attachment: self.attachment.clone(),
            paths: self.paths.clone(),
            language_preference: self.language_preference.clone(),
            permissions: self.permissions.clone(),
            web_search: self.web_search.clone(),
            web_fetch: self.web_fetch.clone(),
            hooks: self.hooks.clone(),
            otel: self.otel.clone(),
            output_style: self.output_style.clone(),
        }
    }

    /// Resolve models with profile override.
    fn resolve_models(&self, profile: Option<&ConfigProfile>) -> ModelRoles {
        let mut models = self.models.clone().unwrap_or_default();

        if let Some(profile_models) = profile.and_then(|p| p.models.as_ref()) {
            models.merge(profile_models);
        }

        models
    }

    /// Get the currently selected profile (if any).
    pub fn selected_profile(&self) -> Option<&ConfigProfile> {
        self.profile
            .as_ref()
            .and_then(|name| self.profiles.get(name))
    }

    /// Resolve logging config with profile override.
    fn resolve_logging(&self, profile: Option<&ConfigProfile>) -> Option<LoggingConfig> {
        match (profile.and_then(|p| p.logging.clone()), &self.logging) {
            (Some(profile_logging), Some(base)) => Some(merge_logging(base, &profile_logging)),
            (Some(profile_logging), None) => Some(profile_logging),
            (None, base) => base.clone(),
        }
    }

    /// Resolve features with profile override.
    fn resolve_features_with_profile(&self, profile: Option<&ConfigProfile>) -> Features {
        let base = self.resolve_features();
        if let Some(profile_features) = profile.and_then(|p| p.features.as_ref()) {
            let mut merged = base;
            merged.apply_map(&profile_features.entries);
            merged
        } else {
            base
        }
    }

    /// Resolve features to runtime type (without profile).
    ///
    /// Returns the configured features merged with defaults, or just defaults
    /// if no features section is present.
    pub fn resolve_features(&self) -> Features {
        self.features
            .clone()
            .map(|f| f.into_features())
            .unwrap_or_else(Features::with_defaults)
    }

    /// List all available profile names.
    pub fn list_profiles(&self) -> Vec<&str> {
        self.profiles.keys().map(String::as_str).collect()
    }

    /// Check if a profile exists.
    pub fn has_profile(&self, name: &str) -> bool {
        self.profiles.contains_key(name)
    }
}

/// Merge two LoggingConfig instances (profile overrides base).
fn merge_logging(base: &LoggingConfig, profile: &LoggingConfig) -> LoggingConfig {
    LoggingConfig {
        level: profile.level.clone().or_else(|| base.level.clone()),
        location: profile.location.or(base.location),
        target: profile.target.or(base.target),
        timezone: profile.timezone.clone().or_else(|| base.timezone.clone()),
        modules: profile.modules.clone().or_else(|| base.modules.clone()),
    }
}

/// Logging configuration section.
///
/// # Example
///
/// ```json
/// {
///   "logging": {
///     "level": "debug",
///     "timezone": "local",
///     "modules": ["cocode_core=debug", "cocode_api=trace"],
///     "location": true,
///     "target": false
///   }
/// }
/// ```
///
/// # Note
///
/// Logging destination is determined by the runtime mode:
/// - TUI mode: Logs to `~/.cocode/log/cocode-tui.log`
/// - REPL mode (`--no-tui`): Logs to stderr
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct LoggingConfig {
    /// Log level (e.g., "trace", "debug", "info", "warn", "error").
    #[serde(default)]
    pub level: Option<String>,

    /// Include source location in logs.
    #[serde(default)]
    pub location: Option<bool>,

    /// Include target module path in logs.
    #[serde(default)]
    pub target: Option<bool>,

    /// Timezone for log timestamps ("local" or "utc", default: "local").
    #[serde(default)]
    pub timezone: Option<String>,

    /// Per-module log levels (e.g., ["cocode_core=debug", "cocode_api=trace"]).
    #[serde(default)]
    pub modules: Option<Vec<String>>,
}

impl LoggingConfig {
    /// Convert to `cocode_utils_common::LoggingConfig` for use with the
    /// `configure_fmt_layer!` macro.
    pub fn to_common_logging(&self) -> cocode_utils_common::LoggingConfig {
        cocode_utils_common::LoggingConfig {
            level: self.level.clone().unwrap_or_else(|| "info".to_string()),
            location: self.location.unwrap_or(false),
            target: self.target.unwrap_or(false),
            timezone: match self.timezone.as_deref() {
                Some("utc") => cocode_utils_common::TimezoneConfig::Utc,
                _ => cocode_utils_common::TimezoneConfig::Local,
            },
            modules: self.modules.clone().unwrap_or_default(),
        }
    }
}

/// Feature toggles section in JSON format.
///
/// This type represents the `features` object in config.json.
/// Use `into_features()` to convert to the runtime `Features` type.
///
/// # Example
///
/// ```json
/// {
///   "features": {
///     "web_fetch": true,
///     "web_fetch": true
///   }
/// }
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct FeaturesConfig {
    /// Feature key to enabled/disabled mapping.
    #[serde(flatten)]
    pub entries: BTreeMap<String, bool>,
}

impl FeaturesConfig {
    /// Convert to runtime `Features` type.
    ///
    /// Applies the JSON entries on top of the default feature set.
    pub fn into_features(self) -> cocode_protocol::Features {
        let mut features = cocode_protocol::Features::with_defaults();
        features.apply_map(&self.entries);
        features
    }

    /// Check if a specific feature is set in this JSON config.
    pub fn get(&self, key: &str) -> Option<bool> {
        self.entries.get(key).copied()
    }

    /// Check if any features are configured.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Validate feature keys and return any unknown keys.
    ///
    /// Returns a list of keys that don't match any known feature.
    /// Can be used to warn users about typos in their config.
    pub fn unknown_keys(&self) -> Vec<String> {
        self.entries
            .keys()
            .filter(|k| !cocode_protocol::is_known_feature_key(k))
            .cloned()
            .collect()
    }
}

/// OpenTelemetry configuration section.
///
/// # Example
///
/// ```json
/// {
///   "otel": {
///     "enabled": true,
///     "exporter": "otlp_http",
///     "endpoint": "http://localhost:4318",
///     "event_log_file": true
///   }
/// }
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct OtelJsonConfig {
    /// Enable OTel (defaults to false if not set).
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Environment name (e.g., "production", "staging").
    #[serde(default)]
    pub environment: Option<String>,
    /// Service name (defaults to "cocode").
    #[serde(default)]
    pub service_name: Option<String>,
    /// Log exporter: "none" | "otlp_http" | "otlp_grpc"
    #[serde(default)]
    pub exporter: Option<String>,
    /// Trace exporter: "none" | "otlp_http" | "otlp_grpc"
    #[serde(default)]
    pub trace_exporter: Option<String>,
    /// Metrics exporter: "none" | "otlp_http" | "otlp_grpc"
    #[serde(default)]
    pub metrics_exporter: Option<String>,
    /// OTLP endpoint (overridden by OTEL_EXPORTER_OTLP_ENDPOINT env var).
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Extra headers as key=value pairs.
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    /// Enable file-based event log output to ~/.cocode/log/otel-events.log.
    #[serde(default)]
    pub event_log_file: Option<bool>,
}

impl OtelJsonConfig {
    /// Convert to `OtelSettings` with env var resolution.
    ///
    /// Standard OTel env vars override config values:
    /// - `OTEL_EXPORTER_OTLP_ENDPOINT` overrides `endpoint`
    /// - `OTEL_EXPORTER_OTLP_HEADERS` overrides `headers`
    /// - `OTEL_SERVICE_NAME` overrides `service_name`
    pub fn to_otel_settings(
        &self,
        cocode_home: &std::path::Path,
    ) -> cocode_otel::config::OtelSettings {
        use cocode_otel::config::OtelExporter;
        use cocode_otel::config::OtelHttpProtocol;
        use cocode_otel::config::OtelSettings;

        let service_name = std::env::var("OTEL_SERVICE_NAME")
            .ok()
            .or_else(|| self.service_name.clone())
            .unwrap_or_else(|| "cocode".to_string());

        let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .ok()
            .or_else(|| self.endpoint.clone())
            .unwrap_or_else(|| "http://localhost:4318".to_string());

        let headers = std::env::var("OTEL_EXPORTER_OTLP_HEADERS")
            .ok()
            .map(|h| {
                h.split(',')
                    .filter_map(|pair| {
                        let mut parts = pair.splitn(2, '=');
                        let key = parts.next()?.trim().to_string();
                        let value = parts.next()?.trim().to_string();
                        Some((key, value))
                    })
                    .collect()
            })
            .or_else(|| self.headers.clone())
            .unwrap_or_default();

        let environment = self
            .environment
            .clone()
            .unwrap_or_else(|| "development".to_string());

        let parse_exporter = |name: Option<&str>| -> OtelExporter {
            match name {
                Some("otlp_http") => OtelExporter::OtlpHttp {
                    endpoint: endpoint.clone(),
                    headers: headers.clone(),
                    protocol: OtelHttpProtocol::Binary,
                    tls: None,
                },
                Some("otlp_grpc") => OtelExporter::OtlpGrpc {
                    endpoint: endpoint.clone(),
                    headers: headers.clone(),
                    tls: None,
                },
                _ => OtelExporter::None,
            }
        };

        OtelSettings {
            environment,
            service_name,
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            home_dir: cocode_home.to_path_buf(),
            exporter: parse_exporter(self.exporter.as_deref()),
            trace_exporter: parse_exporter(self.trace_exporter.as_deref()),
            metrics_exporter: parse_exporter(self.metrics_exporter.as_deref()),
        }
    }

    /// Check if OTel should be enabled based on this config.
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(false)
    }
}

#[cfg(test)]
#[path = "json_config.test.rs"]
mod tests;
