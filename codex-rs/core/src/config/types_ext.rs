//! Types used to define the fields of [`crate::config::Config`].

// Note this file should generally be restricted to simple struct/enum
// definitions that do not contain business logic.

use serde::Deserialize;
use serde::Serialize;

/// Logging configuration for tracing subscriber
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LoggingConfig {
    /// Show file name and line number in log output
    pub location: bool,

    /// Show module path (target) in log output
    pub target: bool,

    /// Timezone for log timestamps
    pub timezone: TimezoneConfig,

    /// Default log level (trace, debug, info, warn, error)
    pub level: String,

    /// Module-specific log levels (e.g., "codex_core=debug,codex_tui=info")
    #[serde(default)]
    pub modules: Vec<String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            location: false,                 // Don't show file/line by default (keep logs clean)
            target: false,                   // Don't show module path by default
            timezone: TimezoneConfig::Local, // Use local timezone by default
            level: "info".to_string(),
            modules: vec![],
        }
    }
}

/// Timezone configuration for log timestamps
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TimezoneConfig {
    /// Use local timezone
    Local,
    /// Use UTC timezone
    Utc,
}

impl Default for TimezoneConfig {
    fn default() -> Self {
        Self::Local
    }
}
