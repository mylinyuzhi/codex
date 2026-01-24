//! Error types for configuration management.

use thiserror::Error;

/// Configuration error type.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Home directory not found.
    #[error("Home directory not found")]
    HomeDirNotFound,

    /// Configuration file not found.
    #[error("Config file not found: {0}")]
    FileNotFound(String),

    /// Invalid JSON in configuration file.
    #[error("Invalid JSON in {file}: {error}")]
    InvalidJson {
        /// The file path.
        file: String,
        /// The error message.
        error: String,
    },

    /// Provider not found.
    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    /// Model not found.
    #[error("Model not found: {0}")]
    ModelNotFound(String),

    /// Profile not found.
    #[error("Profile not found: {0}")]
    ProfileNotFound(String),

    /// Authentication failed (e.g., API key not found).
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Internal error (e.g., lock acquisition failure).
    #[error("Internal error: {0}")]
    Internal(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Result type alias for configuration operations.
pub type Result<T> = std::result::Result<T, ConfigError>;
