//! Plugin error taxonomy mirroring TS `types/plugin.ts` (20+ variants).
//!
//! TS source: `types/plugin.ts` discriminated union for `PluginError`.
//!
//! Used by Layer-2/3 refresh, dependency resolver, manifest loader. Each
//! variant carries structured fields so the UI can render specific error
//! cards and OTel can break down by `error.kind`.

use crate::dependency::DemotionReason;
use crate::identifier::PluginId;
use std::path::PathBuf;
use thiserror::Error;

/// Source attribution for an error (which subsystem produced it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorSource {
    Manifest,
    Loader,
    Marketplace,
    Resolver,
    Hooks,
    Mcp,
    Lsp,
    Mcpb,
    Generic(String),
}

impl ErrorSource {
    pub fn as_str(&self) -> &str {
        match self {
            ErrorSource::Manifest => "manifest",
            ErrorSource::Loader => "loader",
            ErrorSource::Marketplace => "marketplace",
            ErrorSource::Resolver => "resolver",
            ErrorSource::Hooks => "hooks",
            ErrorSource::Mcp => "mcp",
            ErrorSource::Lsp => "lsp",
            ErrorSource::Mcpb => "mcpb",
            ErrorSource::Generic(s) => s,
        }
    }
}

/// Unified plugin error.
///
/// TS: 21 named variants in `types/plugin.ts PluginError`. Variants below
/// cover the same surface; non-static fields use String for stability.
#[derive(Debug, Clone, Error)]
pub enum PluginError {
    #[error("git auth failed for {url}: {message}")]
    GitAuthFailed { url: String, message: String },
    #[error("git clone failed for {url}: {message}")]
    GitCloneFailed { url: String, message: String },
    #[error("network error fetching {url}: {message}")]
    NetworkError { url: String, message: String },
    #[error("download failed from {url} (status {status})")]
    DownloadFailed { url: String, status: i32 },
    #[error("npm install failed for {package}: {message}")]
    NpmInstallFailed { package: String, message: String },
    #[error("pip install failed for {package}: {message}")]
    PipInstallFailed { package: String, message: String },
    #[error("archive extract failed at {path}: {message}")]
    ArchiveExtractFailed { path: PathBuf, message: String },

    #[error("manifest parse error at {path}: {message}")]
    ManifestParseError { path: PathBuf, message: String },
    #[error("manifest not found at {path}")]
    ManifestNotFound { path: PathBuf },
    #[error("manifest validation failed for {name}: {reason}")]
    ManifestValidationFailed { name: String, reason: String },

    #[error("dependency cycle detected: {}", cycle.iter().map(PluginId::to_string).collect::<Vec<_>>().join(" → "))]
    DependencyCycle { cycle: Vec<PluginId> },
    #[error("dependency unsatisfied: {plugin} → {dependency} ({reason:?})")]
    DependencyUnsatisfied {
        plugin: PluginId,
        dependency: PluginId,
        reason: DemotionReason,
    },
    #[error("cross-marketplace dependency: {plugin} → {dep} (marketplace={marketplace})")]
    CrossMarketplaceDependency {
        plugin: PluginId,
        dep: PluginId,
        marketplace: String,
    },

    #[error("path traversal in {name}: {path}")]
    PathTraversal { name: String, path: PathBuf },
    #[error("plugin {name} impersonates official pattern {pattern}")]
    Impersonation { name: String, pattern: String },
    #[error("plugin {name} blocked by policy: {reason}")]
    BlockedByPolicy { name: String, reason: String },

    #[error("contribution conflict for {kind}/{name}: contributors {plugins:?}")]
    ContributionConflict {
        kind: String,
        name: String,
        plugins: Vec<PluginId>,
    },
    #[error("plugin not found: {name}")]
    PluginNotFound { name: String },
    #[error("version mismatch for {name}: expected {expected}, got {actual}")]
    VersionMismatch {
        name: String,
        expected: String,
        actual: String,
    },
    #[error("cache directory error at {path}: {message}")]
    CacheDirError { path: PathBuf, message: String },
    #[error("migration failed v{from}→v{to}: {message}")]
    MigrationFailed { from: i32, to: i32, message: String },

    #[error("hook load failed: {message}")]
    HookLoadFailed { message: String },
    #[error("MCPB load failed: {message}")]
    McpbLoadFailed { message: String },

    #[error("generic plugin error from {origin}: {message}")]
    Generic { origin: String, message: String },
}

impl PluginError {
    /// Telemetry-friendly category name.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::GitAuthFailed { .. } => "git_auth_failed",
            Self::GitCloneFailed { .. } => "git_clone_failed",
            Self::NetworkError { .. } => "network_error",
            Self::DownloadFailed { .. } => "download_failed",
            Self::NpmInstallFailed { .. } => "npm_install_failed",
            Self::PipInstallFailed { .. } => "pip_install_failed",
            Self::ArchiveExtractFailed { .. } => "archive_extract_failed",
            Self::ManifestParseError { .. } => "manifest_parse_error",
            Self::ManifestNotFound { .. } => "manifest_not_found",
            Self::ManifestValidationFailed { .. } => "manifest_validation_failed",
            Self::DependencyCycle { .. } => "dependency_cycle",
            Self::DependencyUnsatisfied { .. } => "dependency_unsatisfied",
            Self::CrossMarketplaceDependency { .. } => "cross_marketplace_dependency",
            Self::PathTraversal { .. } => "path_traversal",
            Self::Impersonation { .. } => "impersonation",
            Self::BlockedByPolicy { .. } => "blocked_by_policy",
            Self::ContributionConflict { .. } => "contribution_conflict",
            Self::PluginNotFound { .. } => "plugin_not_found",
            Self::VersionMismatch { .. } => "version_mismatch",
            Self::CacheDirError { .. } => "cache_dir_error",
            Self::MigrationFailed { .. } => "migration_failed",
            Self::HookLoadFailed { .. } => "hook_load_failed",
            Self::McpbLoadFailed { .. } => "mcpb_load_failed",
            Self::Generic { .. } => "generic",
        }
    }
}
