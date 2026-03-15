//! Error types for the plugin system.

use std::path::PathBuf;

use cocode_error::ErrorExt;
use cocode_error::Location;
use cocode_error::StatusCode;
use cocode_error::stack_trace_debug;
use snafu::Snafu;

/// Plugin errors.
#[stack_trace_debug]
#[derive(Snafu)]
#[snafu(visibility(pub(crate)), module)]
pub enum PluginError {
    /// Plugin manifest not found.
    #[snafu(display("Plugin manifest not found: {}", path.display()))]
    ManifestNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: Location,
    },

    /// Invalid plugin manifest.
    #[snafu(display("Invalid plugin manifest at {}: {message}", path.display()))]
    InvalidManifest {
        path: PathBuf,
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Plugin already registered.
    #[snafu(display("Plugin already registered: {name}"))]
    AlreadyRegistered {
        name: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Plugin not found.
    #[snafu(display("Plugin not found: {name}"))]
    NotFound {
        name: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// IO error during plugin loading.
    #[snafu(display("IO error at {}: {message}", path.display()))]
    Io {
        path: PathBuf,
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Path traversal attempted.
    #[snafu(display("Path traversal not allowed: {}", path.display()))]
    PathTraversal {
        path: PathBuf,
        #[snafu(implicit)]
        location: Location,
    },

    /// Invalid version format.
    #[snafu(display("Invalid version format: {version}"))]
    InvalidVersion {
        version: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Marketplace not found.
    #[snafu(display("Marketplace not found: {name}"))]
    MarketplaceNotFound {
        name: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Marketplace already exists.
    #[snafu(display("Marketplace already exists: {name}"))]
    MarketplaceAlreadyExists {
        name: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Plugin not installed.
    #[snafu(display("Plugin not installed: {plugin_id}"))]
    PluginNotInstalled {
        plugin_id: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Installation failed.
    #[snafu(display("Installation failed for {plugin_id}: {message}"))]
    InstallationFailed {
        plugin_id: String,
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Git clone failed.
    #[snafu(display("Git clone failed for {url}: {message}"))]
    GitCloneFailed {
        url: String,
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Cache error.
    #[snafu(display("Cache error at {}: {message}", path.display()))]
    CacheError {
        path: PathBuf,
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Registry corrupted.
    #[snafu(display("Registry corrupted at {}: {message}", path.display()))]
    RegistryCorrupted {
        path: PathBuf,
        message: String,
        #[snafu(implicit)]
        location: Location,
    },
}

impl ErrorExt for PluginError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::ManifestNotFound { .. } => StatusCode::FileNotFound,
            Self::InvalidManifest { .. } => StatusCode::InvalidConfig,
            Self::AlreadyRegistered { .. } => StatusCode::InvalidArguments,
            Self::NotFound { .. } => StatusCode::FileNotFound,
            Self::Io { .. } => StatusCode::IoError,
            Self::PathTraversal { .. } => StatusCode::PermissionDenied,
            Self::InvalidVersion { .. } => StatusCode::InvalidConfig,
            Self::MarketplaceNotFound { .. } => StatusCode::FileNotFound,
            Self::MarketplaceAlreadyExists { .. } => StatusCode::InvalidArguments,
            Self::PluginNotInstalled { .. } => StatusCode::FileNotFound,
            Self::InstallationFailed { .. } => StatusCode::IoError,
            Self::GitCloneFailed { .. } => StatusCode::IoError,
            Self::CacheError { .. } => StatusCode::IoError,
            Self::RegistryCorrupted { .. } => StatusCode::InvalidConfig,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Result type for plugin operations.
pub type Result<T> = std::result::Result<T, PluginError>;

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
