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

/// Retrieval system configuration for code search.
///
/// This is the TOML-friendly version. When code_search feature is enabled,
/// it gets converted to `codex_retrieval::RetrievalConfig`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RetrievalConfigToml {
    /// Whether retrieval is enabled
    pub enabled: bool,

    /// Directory for storing index data (default: ~/.codex/retrieval)
    pub data_dir: Option<String>,

    /// Maximum file size in MB to index (default: 5)
    pub max_file_size_mb: i32,

    /// Maximum chunk size in characters (default: 512)
    pub max_chunk_size: i32,

    /// Number of final results to return (default: 20)
    pub n_final: i32,

    /// Enable query expansion with synonyms (default: false)
    pub enable_expansion: bool,
}

impl Default for RetrievalConfigToml {
    fn default() -> Self {
        Self {
            enabled: false,
            data_dir: None,
            max_file_size_mb: 5,
            max_chunk_size: 512,
            n_final: 20,
            enable_expansion: false,
        }
    }
}

impl RetrievalConfigToml {
    /// Convert to the full RetrievalConfig from the retrieval crate.
    pub fn to_retrieval_config(&self) -> codex_retrieval::RetrievalConfig {
        use std::path::PathBuf;

        let mut config = codex_retrieval::RetrievalConfig::default();
        config.enabled = self.enabled;

        if let Some(ref dir) = self.data_dir {
            // Handle ~ expansion manually
            let expanded = if dir.starts_with("~/") {
                if let Some(home) = dirs::home_dir() {
                    home.join(&dir[2..])
                } else {
                    PathBuf::from(dir)
                }
            } else {
                PathBuf::from(dir)
            };
            config.data_dir = expanded;
        }

        config.indexing.max_file_size_mb = self.max_file_size_mb;
        config.chunking.max_chunk_size = self.max_chunk_size;
        config.search.n_final = self.n_final;

        config
    }
}
