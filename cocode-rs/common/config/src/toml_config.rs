//! TOML configuration types for config.toml.
//!
//! This module defines the file format types for `~/.cocode/config.toml`.
//! These types represent the TOML structure and are separate from the runtime
//! feature types in `cocode_protocol::features`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// TOML configuration file (~/.cocode/config.toml).
///
/// # Example
///
/// ```toml
/// model_provider = "genai"
/// model = "gemini-3-pro-preview-new"
/// model_max_output_tokens = 12000
///
/// [logging]
/// level = "debug"
/// location = true
/// target = false
///
/// [features]
/// subagent = true
/// compact_v2 = true
/// web_fetch = true
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct ConfigToml {
    /// Model provider name (e.g., "openai", "anthropic", "genai").
    #[serde(default)]
    pub model_provider: Option<String>,

    /// Model ID (e.g., "gpt-5", "claude-opus-4").
    #[serde(default)]
    pub model: Option<String>,

    /// Profile name to use.
    #[serde(default)]
    pub profile: Option<String>,

    /// Maximum output tokens for model responses.
    #[serde(default)]
    pub model_max_output_tokens: Option<i32>,

    /// Logging configuration.
    #[serde(default)]
    pub logging: Option<LoggingConfig>,

    /// Feature toggles.
    #[serde(default)]
    pub features: Option<FeaturesToml>,
}

/// Logging configuration section.
///
/// # Example
///
/// ```toml
/// [logging]
/// level = "debug"
/// location = true
/// target = false
/// ```
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
}

/// Feature toggles section in TOML format.
///
/// This type represents the `[features]` table in config.toml.
/// Use `into_features()` to convert to the runtime `Features` type.
///
/// # Example
///
/// ```toml
/// [features]
/// subagent = true
/// web_fetch = true
/// shell_tool = false
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct FeaturesToml {
    /// Feature key to enabled/disabled mapping.
    #[serde(flatten)]
    pub entries: BTreeMap<String, bool>,
}

impl FeaturesToml {
    /// Convert to runtime `Features` type.
    ///
    /// Applies the TOML entries on top of the default feature set.
    pub fn into_features(self) -> cocode_protocol::Features {
        let mut features = cocode_protocol::Features::with_defaults();
        features.apply_map(&self.entries);
        features
    }

    /// Check if a specific feature is set in this TOML config.
    pub fn get(&self, key: &str) -> Option<bool> {
        self.entries.get(key).copied()
    }

    /// Check if any features are configured.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_toml_default() {
        let config = ConfigToml::default();
        assert!(config.model_provider.is_none());
        assert!(config.model.is_none());
        assert!(config.profile.is_none());
        assert!(config.logging.is_none());
        assert!(config.features.is_none());
    }

    #[test]
    fn test_config_toml_parse_minimal() {
        let toml_str = r#"
model_provider = "openai"
model = "gpt-5"
"#;
        let config: ConfigToml = toml::from_str(toml_str).unwrap();
        assert_eq!(config.model_provider, Some("openai".to_string()));
        assert_eq!(config.model, Some("gpt-5".to_string()));
    }

    #[test]
    fn test_config_toml_parse_full() {
        let toml_str = r#"
model_provider = "genai"
model = "gemini-3-pro"
profile = "coding"
model_max_output_tokens = 12000

[logging]
level = "debug"
location = true
target = false

[features]
subagent = true
web_fetch = true
shell_tool = false
"#;
        let config: ConfigToml = toml::from_str(toml_str).unwrap();
        assert_eq!(config.model_provider, Some("genai".to_string()));
        assert_eq!(config.model, Some("gemini-3-pro".to_string()));
        assert_eq!(config.profile, Some("coding".to_string()));
        assert_eq!(config.model_max_output_tokens, Some(12000));

        let logging = config.logging.unwrap();
        assert_eq!(logging.level, Some("debug".to_string()));
        assert_eq!(logging.location, Some(true));
        assert_eq!(logging.target, Some(false));

        let features = config.features.unwrap();
        assert_eq!(features.get("subagent"), Some(true));
        assert_eq!(features.get("web_fetch"), Some(true));
        assert_eq!(features.get("shell_tool"), Some(false));
    }

    #[test]
    fn test_features_toml_into_features() {
        let mut entries = BTreeMap::new();
        entries.insert("subagent".to_string(), true);
        entries.insert("shell_tool".to_string(), false);

        let features_toml = FeaturesToml { entries };
        let features = features_toml.into_features();

        // subagent should be enabled (it was set to true)
        assert!(features.enabled(cocode_protocol::Feature::Subagent));
        // shell_tool should be disabled (it was set to false, overriding default true)
        assert!(!features.enabled(cocode_protocol::Feature::ShellTool));
    }

    #[test]
    fn test_logging_config_default() {
        let config = LoggingConfig::default();
        assert!(config.level.is_none());
        assert!(config.location.is_none());
        assert!(config.target.is_none());
    }
}
