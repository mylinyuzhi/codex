//! Permission checking for sandbox-enforced operations.

use std::path::Path;

use crate::config::EnforcementLevel;
use crate::config::SandboxConfig;
use crate::error::Result;
use crate::error::sandbox_error::*;

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
    /// Cross-cutting checks (applied before enforcement level logic):
    /// - `denied_read_paths` blocks reads to specific paths
    /// - `deny_write_paths` blocks writes to specific paths
    /// - `allow_git_config` controls write access to git config files
    ///
    /// Enforcement levels:
    /// - `Disabled`: all paths allowed (cross-cutting checks still apply)
    /// - `ReadOnly`: reads allowed; writes denied
    /// - `WorkspaceWrite`: reads allowed; writes only to writable roots
    /// - `Strict`: path must be under a root; writes require writable root
    pub fn check_path(&self, path: &Path, write: bool) -> Result<()> {
        let op = if write { "write" } else { "read" };

        // Cross-cutting: deny-read paths block reads in all non-disabled modes
        if self.config.enforcement != EnforcementLevel::Disabled
            && !write
            && self.is_denied_read_path(path)
        {
            tracing::info!(
                path = %path.display(),
                operation = op,
                enforcement = ?self.config.enforcement,
                decision = "denied",
                reason = "deny_read rule",
                "sandbox.permission_check"
            );
            return PathDeniedSnafu {
                path: path.display().to_string(),
            }
            .fail();
        }

        // Cross-cutting: deny-write paths block writes in all non-disabled modes
        if self.config.enforcement != EnforcementLevel::Disabled
            && write
            && self.is_deny_write_path(path)
        {
            tracing::info!(
                path = %path.display(),
                operation = op,
                enforcement = ?self.config.enforcement,
                decision = "denied",
                reason = "deny_write rule",
                "sandbox.permission_check"
            );
            return WriteDeniedSnafu {
                message: format!("write denied by deny_write rule: {}", path.display()),
            }
            .fail();
        }

        // Cross-cutting: git config protection
        if self.config.enforcement != EnforcementLevel::Disabled
            && write
            && !self.config.allow_git_config
            && is_git_config_path(path)
        {
            tracing::info!(
                path = %path.display(),
                operation = op,
                enforcement = ?self.config.enforcement,
                decision = "denied",
                reason = "git config protection",
                "sandbox.permission_check"
            );
            return WriteDeniedSnafu {
                message: format!(
                    "writing to git config is not allowed (allow_git_config=false): {}",
                    path.display()
                ),
            }
            .fail();
        }

        match self.config.enforcement {
            EnforcementLevel::Disabled => Ok(()),
            EnforcementLevel::ReadOnly => {
                if write {
                    tracing::info!(
                        path = %path.display(),
                        operation = op,
                        enforcement = "read_only",
                        decision = "denied",
                        reason = "read-only mode",
                        "sandbox.permission_check"
                    );
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
            EnforcementLevel::WorkspaceWrite => {
                if write {
                    if self.is_denied_path(path) {
                        tracing::info!(
                            path = %path.display(),
                            operation = op,
                            enforcement = "workspace_write",
                            decision = "denied",
                            reason = "denied path",
                            "sandbox.permission_check"
                        );
                        return PathDeniedSnafu {
                            path: path.display().to_string(),
                        }
                        .fail();
                    }
                    if !self.is_writable_path(path) {
                        tracing::info!(
                            path = %path.display(),
                            operation = op,
                            enforcement = "workspace_write",
                            decision = "denied",
                            reason = "outside writable roots",
                            "sandbox.permission_check"
                        );
                        return WriteDeniedSnafu {
                            message: format!(
                                "path is not under any writable root: {}",
                                path.display()
                            ),
                        }
                        .fail();
                    }
                }
                Ok(())
            }
            EnforcementLevel::Strict => {
                if self.is_denied_path(path) {
                    tracing::info!(
                        path = %path.display(),
                        operation = op,
                        enforcement = "strict",
                        decision = "denied",
                        reason = "denied path",
                        "sandbox.permission_check"
                    );
                    return PathDeniedSnafu {
                        path: path.display().to_string(),
                    }
                    .fail();
                }

                if !self.is_under_any_root(path) {
                    tracing::info!(
                        path = %path.display(),
                        operation = op,
                        enforcement = "strict",
                        decision = "denied",
                        reason = "outside all roots",
                        "sandbox.permission_check"
                    );
                    return PathDeniedSnafu {
                        path: path.display().to_string(),
                    }
                    .fail();
                }

                if write && !self.is_writable_path(path) {
                    tracing::info!(
                        path = %path.display(),
                        operation = op,
                        enforcement = "strict",
                        decision = "denied",
                        reason = "read-only subpath",
                        "sandbox.permission_check"
                    );
                    return WriteDeniedSnafu {
                        message: format!("path is under a read-only subpath: {}", path.display()),
                    }
                    .fail();
                }

                Ok(())
            }
        }
    }

    /// Checks whether network access is allowed.
    pub fn check_network(&self) -> Result<()> {
        if self.config.enforcement == EnforcementLevel::Disabled {
            return Ok(());
        }

        if !self.config.allow_network {
            tracing::info!(
                enforcement = ?self.config.enforcement,
                decision = "denied",
                reason = "network access disabled",
                "sandbox.network_check"
            );
            return NetworkDeniedSnafu.fail();
        }

        Ok(())
    }

    /// Returns true if the path is under any writable root (respecting subpath restrictions).
    pub fn is_writable_path(&self, path: &Path) -> bool {
        self.config
            .writable_roots
            .iter()
            .any(|root| root.is_writable(path))
    }

    /// Returns true if the path is under any writable root (ignoring subpath restrictions).
    ///
    /// In `Disabled` mode with no configured roots, returns `true` (everything accessible).
    /// In other modes with no roots, returns `false` (nothing accessible).
    pub fn is_under_any_root(&self, path: &Path) -> bool {
        if self.config.writable_roots.is_empty() {
            return self.config.enforcement == EnforcementLevel::Disabled;
        }
        self.config
            .writable_roots
            .iter()
            .any(|root| root.contains(path))
    }

    /// Returns true if the path is under one of the denied paths.
    fn is_denied_path(&self, path: &Path) -> bool {
        self.config
            .denied_paths
            .iter()
            .any(|denied| path.starts_with(denied))
    }

    /// Returns true if the path is under a deny-read path.
    fn is_denied_read_path(&self, path: &Path) -> bool {
        self.config
            .denied_read_paths
            .iter()
            .any(|denied| path.starts_with(denied))
    }

    /// Returns true if the path is under a deny-write path.
    fn is_deny_write_path(&self, path: &Path) -> bool {
        self.config
            .deny_write_paths
            .iter()
            .any(|denied| path.starts_with(denied))
    }
}

/// Check if a path is a git config file.
fn is_git_config_path(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    path_str.ends_with("/.git/config")
        || path_str.ends_with("/.gitconfig")
        || path_str.contains("/.git/config/")
}

#[cfg(test)]
#[path = "checker.test.rs"]
mod tests;
