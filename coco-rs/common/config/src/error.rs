//! Typed errors for resolution and validation.
//!
//! Used by `PartialProviderConfig::from_partial`,
//! `PartialModelInfo::from_partial`, `PositiveTokens::try_from`, and
//! `build_model_registry`. Surface-level callers convert into `anyhow`
//! at the runtime-config builder boundary.

use std::fmt;
use std::path::PathBuf;
use thiserror::Error;

/// Required field on a partial provider or model overlay. Closed set
/// so error reporting can branch / format without `&'static str`
/// churn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigField {
    // Provider-level
    Api,
    EnvKey,
    BaseUrl,
    // Model-level
    ContextWindow,
    MaxOutputTokens,
}

impl ConfigField {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::EnvKey => "env_key",
            Self::BaseUrl => "base_url",
            Self::ContextWindow => "context_window",
            Self::MaxOutputTokens => "max_output_tokens",
        }
    }
}

impl fmt::Display for ConfigField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error(
        "provider `{name}`: missing required field `{field}` (settings.json overlay must declare it for new providers)"
    )]
    IncompleteProviderEntry { name: String, field: ConfigField },

    #[error(
        "model `{provider}/{model}`: missing required field `{field}` — declare it in `models.json` or `providers.<name>.models.<id>`"
    )]
    IncompleteModelEntry {
        provider: String,
        model: String,
        field: ConfigField,
    },

    #[error(
        "non-positive token value {value} — `context_window` / `max_output_tokens` must be a positive integer that fits in u32"
    )]
    NonPositiveTokens { value: i64 },

    #[error(
        "non-positive count value {value} — `top_k` and similar fields must be a positive integer that fits in u32"
    )]
    NonPositiveCount { value: i64 },

    #[error("failed to read base_instructions_file at {path}: {source}")]
    BaseInstructionsRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read {path}: {source}")]
    CatalogRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse {path}: {source}")]
    CatalogParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("unknown provider `{name}` referenced by role binding")]
    UnknownProvider { name: String },

    #[error(
        "unknown model `{provider}/{model}` — not in builtin registry, models.json, or per-provider models"
    )]
    UnknownModel { provider: String, model: String },

    #[error(
        "provider `{name}`: invalid timeout_secs {value} — must be >= 0 (use 0 to disable per-request timeout)"
    )]
    InvalidTimeoutSecs { name: String, value: i64 },
}
