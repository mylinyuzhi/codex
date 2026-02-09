//! Permission checking for sandbox-enforced operations.

use std::path::Path;

use crate::config::SandboxConfig;
use crate::config::SandboxMode;
use crate::error::Result;
use crate::error::sandbox_error::*;
use crate::error::{self};

/// Checks permissions against the sandbox configuration.
#[derive(Debug, Clone)]
pub struct PermissionChecker {
    config: SandboxConfig,
}

impl PermissionChecker {
    /// Creates a new checker with the given configuration.
    pub fn new(config: SandboxConfig) -> Self {
        Self { config }
    }

    /// Returns a reference to the underlying configuration.
    pub fn config(&self) -> &SandboxConfig {
        &self.config
    }

    /// Checks whether the given path is accessible.
    ///
    /// - In `None` mode, all paths are allowed.
    /// - In `ReadOnly` mode, read access is always allowed; write access is denied.
    /// - In `Strict` mode, the path must be explicitly in `allowed_paths` and not in
    ///   `denied_paths`. Write access requires the path to be allowed.
    pub fn check_path(&self, path: &Path, write: bool) -> Result<()> {
        match self.config.mode {
            SandboxMode::None => Ok(()),
            SandboxMode::ReadOnly => {
                if write {
                    return WriteDeniedSnafu {
                        message: format!(
                            "sandbox is in read-only mode, cannot write to: {}",
                            path.display()
                        ),
                    }
                    .fail();
                }
                Ok(())
            }
            SandboxMode::Strict => {
                // Check denied paths first (takes precedence)
                if self.is_denied_path(path) {
                    return PathDeniedSnafu {
                        path: path.display().to_string(),
                    }
                    .fail();
                }

                // In strict mode, the path must be explicitly allowed
                if !self.is_allowed_path(path) {
                    return PathDeniedSnafu {
                        path: path.display().to_string(),
                    }
                    .fail();
                }

                // Write access requires the path to be allowed (already checked above)
                if write && !self.config.mode_allows_write() {
                    return WriteDeniedSnafu {
                        message: format!("write denied in strict mode: {}", path.display()),
                    }
                    .fail();
                }

                Ok(())
            }
        }
    }

    /// Checks whether network access is allowed.
    pub fn check_network(&self) -> Result<()> {
        if self.config.mode == SandboxMode::None {
            return Ok(());
        }

        if !self.config.allow_network {
            return error::sandbox_error::NetworkDeniedSnafu.fail();
        }

        Ok(())
    }

    /// Returns true if the path is under one of the allowed paths.
    pub fn is_allowed_path(&self, path: &Path) -> bool {
        if self.config.allowed_paths.is_empty() {
            // If no allowed paths are configured, allow all (for None/ReadOnly modes)
            return self.config.mode != SandboxMode::Strict;
        }

        self.config
            .allowed_paths
            .iter()
            .any(|allowed| path.starts_with(allowed))
    }

    /// Returns true if the path is under one of the denied paths.
    fn is_denied_path(&self, path: &Path) -> bool {
        self.config
            .denied_paths
            .iter()
            .any(|denied| path.starts_with(denied))
    }
}

/// Extension trait for SandboxConfig to check write permissions.
trait SandboxConfigExt {
    fn mode_allows_write(&self) -> bool;
}

impl SandboxConfigExt for SandboxConfig {
    fn mode_allows_write(&self) -> bool {
        // In strict mode, writes are allowed to explicitly allowed paths
        // None mode allows all, ReadOnly denies all writes
        !matches!(self.mode, SandboxMode::ReadOnly)
    }
}

#[cfg(test)]
#[path = "checker.test.rs"]
mod tests;
