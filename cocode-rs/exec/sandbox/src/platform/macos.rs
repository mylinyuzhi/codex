//! macOS sandbox implementation using Seatbelt (sandbox-exec).
//!
//! Generates SBPL (Seatbelt Profile Language) profiles at runtime based on
//! the sandbox configuration, then wraps commands with `sandbox-exec`.
//! Uses a static base policy (Chrome-inspired, from codex-rs) plus dynamic
//! path rules for writable roots and network access.

use tracing::info;

use crate::config::EnforcementLevel;
use crate::config::SandboxConfig;
use crate::error::Result;
use crate::platform::SandboxPlatform;

/// Path to the system sandbox-exec binary.
/// Hardcoded to /usr/bin to defend against PATH injection.
const SANDBOX_EXEC_PATH: &str = "/usr/bin/sandbox-exec";

/// Static base policy: process, sysctl, iokit, mach, pty rules.
/// Inspired by Chrome's sandbox policy and codex-rs seatbelt_base_policy.sbpl.
const SEATBELT_BASE_POLICY: &str = include_str!("seatbelt_base.sbpl");

/// Static network policy: TLS services, DNS, AF_SYSTEM sockets.
const SEATBELT_NETWORK_POLICY: &str = include_str!("seatbelt_network.sbpl");

/// macOS Seatbelt sandbox implementation.
///
/// Wraps commands with `sandbox-exec -p <profile>` using a static base
/// policy combined with dynamically generated path rules.
pub struct MacOsSandbox;

impl SandboxPlatform for MacOsSandbox {
    fn available(&self) -> bool {
        cfg!(target_os = "macos") && std::path::Path::new(SANDBOX_EXEC_PATH).exists()
    }

    fn wrap_command(
        &self,
        config: &SandboxConfig,
        cmd: &mut tokio::process::Command,
    ) -> Result<()> {
        if config.enforcement == EnforcementLevel::Disabled {
            return Ok(());
        }

        let profile = generate_seatbelt_profile(config);

        info!(
            enforcement = ?config.enforcement,
            writable_roots = config.writable_roots.len(),
            allow_network = config.allow_network,
            profile_len = profile.len(),
            "Wrapping command with macOS Seatbelt sandbox"
        );

        let inner = cmd.as_std();
        let program = inner.get_program().to_os_string();
        let args: Vec<_> = inner
            .get_args()
            .map(std::ffi::OsStr::to_os_string)
            .collect();

        // Rebuild as: sandbox-exec -p <profile> <original_program> <original_args...>
        *cmd = tokio::process::Command::new(SANDBOX_EXEC_PATH);
        cmd.arg("-p").arg(&profile);
        cmd.arg(&program);
        cmd.args(&args);

        // Process hardening: clear dangerous env vars
        cmd.env_remove("DYLD_INSERT_LIBRARIES");
        cmd.env_remove("DYLD_LIBRARY_PATH");
        cmd.env_remove("DYLD_FRAMEWORK_PATH");

        Ok(())
    }
}

/// Escape a path for safe inclusion in SBPL string literals.
///
/// SBPL uses Lisp-style strings where backslash and double-quote are special.
/// Newlines are stripped to prevent rule injection via multi-line paths.
fn escape_sbpl_path(path: &str) -> String {
    path.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "")
        .replace('\r', "")
}

/// Generate a complete Seatbelt profile from static base + dynamic paths.
fn generate_seatbelt_profile(config: &SandboxConfig) -> String {
    let mut profile = String::with_capacity(4096);

    // Static base policy (process, sysctl, iokit, mach, pty)
    profile.push_str(SEATBELT_BASE_POLICY);
    profile.push('\n');

    // System read access (always allowed for basic operation)
    profile.push_str("; System read access\n");
    for path in &[
        "/usr",
        "/bin",
        "/sbin",
        "/lib",
        "/System/Library",
        "/Library/Frameworks",
        "/dev/null",
        "/dev/urandom",
        "/dev/random",
        "/private/var/tmp",
        "/private/tmp",
        "/etc",
    ] {
        profile.push_str(&format!("(allow file-read* (subpath \"{path}\"))\n"));
    }

    // Home directory read access
    if let Some(home) = dirs_home() {
        let escaped = escape_sbpl_path(&home);
        profile.push_str(&format!("(allow file-read* (subpath \"{escaped}\"))\n"));
    }
    profile.push('\n');

    // Writable roots with subpath protection
    if !config.writable_roots.is_empty() {
        profile.push_str("; Writable roots\n");
        for root in &config.writable_roots {
            let path = escape_sbpl_path(&root.path.display().to_string());
            profile.push_str(&format!("(allow file-write* (subpath \"{path}\"))\n"));
            // Re-deny write access to read-only subpaths
            for sub in &root.readonly_subpaths {
                let sub_path = root.path.join(sub);
                let escaped = escape_sbpl_path(&sub_path.display().to_string());
                profile.push_str(&format!("(deny file-write* (subpath \"{escaped}\"))\n"));
            }
        }
        profile.push('\n');
    }

    // Temp directory write access
    profile.push_str("; Temp directory write access\n");
    profile.push_str("(allow file-write* (subpath \"/tmp\"))\n");
    profile.push_str("(allow file-write* (subpath \"/private/tmp\"))\n");
    if let Ok(tmpdir) = std::env::var("TMPDIR") {
        let escaped = escape_sbpl_path(&tmpdir);
        profile.push_str(&format!("(allow file-write* (subpath \"{escaped}\"))\n"));
    }
    profile.push('\n');

    // Network access
    profile.push_str("; Network access\n");
    if config.allow_network {
        profile.push_str("(allow network*)\n");
        // Include TLS and DNS services policy for full network access
        profile.push_str(SEATBELT_NETWORK_POLICY);
        // macOS cache dir for TLS/DNS caching (codex-rs uses DARWIN_USER_CACHE_DIR param;
        // we resolve at runtime since we use -p inline mode)
        let cache_dir = std::env::var("DARWIN_USER_CACHE_DIR")
            .ok()
            .or_else(|| dirs_home().map(|h| format!("{h}/Library/Caches")));
        if let Some(cache_dir) = cache_dir {
            let escaped = escape_sbpl_path(&cache_dir);
            profile.push_str(&format!("(allow file-write* (subpath \"{escaped}\"))\n"));
        }
    } else {
        // Allow loopback only (for proxy connections)
        profile.push_str("(allow network* (local ip \"localhost:*\"))\n");
        profile.push_str("(allow network* (remote ip \"localhost:*\"))\n");
    }

    // Weaker network isolation: allow trustd.agent for Go TLS cert verification.
    // Go programs (gh, gcloud, terraform, kubectl) need system TLS services
    // because they don't use Apple's SecureTransport for certificate verification.
    if config.weaker_network_isolation {
        profile.push_str("; Weaker network isolation: Go TLS cert verification\n");
        profile.push_str("(allow mach-lookup (global-name \"com.apple.trustd.agent\"))\n");
    }

    // PTY restriction: base policy allows PTY by default; deny when disabled.
    if !config.allow_pty {
        profile.push_str("; PTY access denied by config\n");
        profile.push_str("(deny pseudo-tty)\n");
    }
    profile.push('\n');

    profile
}

/// Get the user's home directory, if available.
fn dirs_home() -> Option<String> {
    std::env::var("HOME").ok()
}

#[cfg(test)]
#[path = "macos.test.rs"]
mod tests;
