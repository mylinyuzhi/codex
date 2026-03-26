//! Sandbox dependency detection.
//!
//! Checks whether required platform-specific binaries are available
//! before attempting to enable sandbox enforcement.

use std::path::PathBuf;

use crate::config::SeccompConfig;

/// Result of checking a single dependency.
#[derive(Debug, Clone)]
pub struct DependencyCheck {
    /// Name of the dependency.
    pub name: &'static str,
    /// Whether the dependency is available.
    pub available: bool,
    /// Path where the dependency was found.
    pub path: Option<PathBuf>,
    /// Whether this dependency is required (vs optional).
    pub required: bool,
}

/// Check all required sandbox dependencies for the current platform.
///
/// Returns a list of checks. All `required` dependencies must be available
/// for sandbox to function correctly.
pub fn check_dependencies() -> Vec<DependencyCheck> {
    check_dependencies_with_seccomp(&SeccompConfig::default())
}

/// Check all dependencies, including optional seccomp checks when configured.
///
/// When `seccomp.bpf_path` is set, adds optional checks for the BPF filter file
/// and the seccomp-apply binary.
pub fn check_dependencies_with_seccomp(seccomp: &SeccompConfig) -> Vec<DependencyCheck> {
    let mut checks = Vec::new();

    if cfg!(target_os = "macos") {
        checks.push(check_binary(
            "sandbox-exec",
            &["/usr/bin/sandbox-exec"],
            /*required=*/ true,
        ));
    }

    if cfg!(target_os = "linux") {
        checks.push(check_binary(
            "bwrap",
            &["/usr/bin/bwrap", "/usr/local/bin/bwrap"],
            /*required=*/ true,
        ));
        checks.push(check_binary(
            "socat",
            &["/usr/bin/socat", "/usr/local/bin/socat"],
            /*required=*/ false, // Optional: needed for network bridge
        ));

        // Seccomp dependencies: optional, only checked when configured
        if seccomp.bpf_path.is_some() {
            checks.push(check_seccomp_bpf(seccomp));
            checks.push(check_seccomp_apply(seccomp));
        }
    }

    checks
}

/// Get names of missing required dependencies.
pub fn missing_required() -> Vec<&'static str> {
    check_dependencies()
        .into_iter()
        .filter(|d| d.required && !d.available)
        .map(|d| d.name)
        .collect()
}

/// Check if all required dependencies are available.
pub fn all_required_available() -> bool {
    check_dependencies()
        .iter()
        .filter(|d| d.required)
        .all(|d| d.available)
}

fn check_binary(name: &'static str, paths: &[&str], required: bool) -> DependencyCheck {
    let found = paths.iter().map(PathBuf::from).find(|p| p.exists());
    DependencyCheck {
        name,
        available: found.is_some(),
        path: found,
        required,
    }
}

/// Check if the configured seccomp BPF filter file exists.
fn check_seccomp_bpf(seccomp: &SeccompConfig) -> DependencyCheck {
    let path = seccomp.bpf_path.clone();
    let available = path.as_ref().is_some_and(|p| p.exists());
    DependencyCheck {
        name: "seccomp-bpf",
        available,
        path,
        required: false, // Optional: sandbox works without seccomp
    }
}

/// Check if the seccomp-apply binary is available.
fn check_seccomp_apply(seccomp: &SeccompConfig) -> DependencyCheck {
    // Check explicit path first, then search well-known locations
    let default_paths: &[&str] = &["/usr/bin/seccomp-apply", "/usr/local/bin/seccomp-apply"];
    let found = seccomp
        .apply_path
        .as_ref()
        .filter(|p| p.exists())
        .cloned()
        .or_else(|| default_paths.iter().map(PathBuf::from).find(|p| p.exists()));
    DependencyCheck {
        name: "seccomp-apply",
        available: found.is_some(),
        path: found,
        required: false, // Optional: sandbox works without seccomp
    }
}

#[cfg(test)]
#[path = "deps.test.rs"]
mod tests;
