//! Shell-level sandbox configuration and decision logic.
//!
//! TS: utils/sandbox/sandbox-adapter.ts, tools/BashTool/shouldUseSandbox.ts
//!
//! This module provides the shell-specific sandbox decision layer. It sits
//! between the general sandbox configuration (in `coco-sandbox`) and the
//! shell executor, deciding per-command whether to apply sandboxing and
//! generating platform-specific sandbox arguments.

use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// Sandbox mode controlling the level of isolation.
///
/// Mirrors `SandboxMode` from `coco-sandbox::config` but is defined
/// locally to avoid a crate dependency. The shell crate only needs
/// the mode + config for decision-making, not the full sandbox runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxMode {
    /// No sandbox — commands run directly.
    #[default]
    None,
    /// Read-only mode — file writes are blocked.
    ReadOnly,
    /// Strict mode — only explicitly allowed paths are accessible.
    Strict,
    /// External sandbox — enforcement delegated to an external tool (Docker, etc.).
    External,
}

impl SandboxMode {
    /// Whether any form of sandboxing is active.
    pub fn is_active(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Whether this mode blocks file writes.
    pub fn blocks_writes(&self) -> bool {
        matches!(self, Self::ReadOnly | Self::Strict)
    }
}

/// Shell sandbox configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SandboxConfig {
    /// The active sandbox mode.
    pub mode: SandboxMode,
    /// Writable directory roots (only effective in Strict mode).
    pub writable_roots: Vec<PathBuf>,
    /// Commands excluded from sandboxing (matched by first token).
    pub excluded_commands: Vec<String>,
    /// Auto-approve bash commands when sandbox is active.
    pub auto_allow_if_sandboxed: bool,
    /// Allow the `dangerously_disable_sandbox` flag.
    pub allow_bypass: bool,
    /// Whether network access is allowed inside the sandbox.
    pub allow_network: bool,
    /// Platform-specific enforcement binary (e.g., `bwrap`, `sandbox-exec`).
    pub platform_binary: Option<String>,
}

impl SandboxConfig {
    /// Create a config with sandbox enabled in strict mode.
    pub fn strict(writable_roots: Vec<PathBuf>) -> Self {
        Self {
            mode: SandboxMode::Strict,
            writable_roots,
            auto_allow_if_sandboxed: true,
            allow_bypass: true,
            ..Default::default()
        }
    }

    /// Create a config with read-only sandbox.
    pub fn read_only() -> Self {
        Self {
            mode: SandboxMode::ReadOnly,
            auto_allow_if_sandboxed: true,
            allow_bypass: true,
            ..Default::default()
        }
    }
}

/// Whether a bypass was requested for a specific command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BypassRequest {
    /// No bypass requested.
    No,
    /// Bypass requested via `dangerously_disable_sandbox` parameter.
    Requested,
}

/// Decision about whether to sandbox a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxDecision {
    /// Run the command without sandboxing.
    Unsandboxed { reason: &'static str },
    /// Run the command in a sandbox with the given mode.
    Sandboxed { mode: SandboxMode },
}

impl SandboxDecision {
    /// Whether this decision applies sandboxing.
    pub fn is_sandboxed(&self) -> bool {
        matches!(self, Self::Sandboxed { .. })
    }
}

/// Decide whether a command should be sandboxed.
///
/// Returns `Unsandboxed` with a reason if sandboxing should be skipped,
/// or `Sandboxed` with the active mode.
///
/// TS: shouldUseSandbox() in tools/BashTool/shouldUseSandbox.ts
pub fn should_sandbox_command(
    config: &SandboxConfig,
    command: &str,
    bypass: BypassRequest,
) -> SandboxDecision {
    if !config.mode.is_active() {
        return SandboxDecision::Unsandboxed {
            reason: "sandbox disabled",
        };
    }

    if matches!(bypass, BypassRequest::Requested) && config.allow_bypass {
        return SandboxDecision::Unsandboxed {
            reason: "bypass requested and allowed",
        };
    }

    let trimmed = command.trim();
    if trimmed.is_empty() {
        return SandboxDecision::Unsandboxed {
            reason: "empty command",
        };
    }

    if is_excluded_command(&config.excluded_commands, trimmed) {
        return SandboxDecision::Unsandboxed {
            reason: "command is excluded from sandboxing",
        };
    }

    SandboxDecision::Sandboxed { mode: config.mode }
}

/// Check if a command matches any exclusion pattern.
///
/// Supports:
/// - Exact first-word match: `"git"` matches `"git status"`
/// - Prefix match: `"npm:*"` matches `"npm install"`
fn is_excluded_command(excluded: &[String], command: &str) -> bool {
    let first_word = command.split_whitespace().next().unwrap_or("");
    // Also extract basename for `/usr/bin/git` -> `git` matching
    let basename = Path::new(first_word)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(first_word);

    excluded.iter().any(|pattern| {
        if let Some(prefix) = pattern.strip_suffix(":*") {
            // Prefix match: "npm:*" matches "npm", "npm install"
            first_word == prefix
                || first_word.starts_with(&format!("{prefix} "))
                || basename == prefix
                || basename.starts_with(&format!("{prefix} "))
        } else {
            // Exact first-word match
            first_word == pattern || basename == pattern
        }
    })
}

/// Platform identifier for sandbox argument generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    MacOs,
}

impl Platform {
    /// Detect the current platform.
    pub fn current() -> Option<Self> {
        if cfg!(target_os = "linux") {
            Some(Self::Linux)
        } else if cfg!(target_os = "macos") {
            Some(Self::MacOs)
        } else {
            None
        }
    }
}

/// Generate platform-specific sandbox arguments for wrapping a command.
///
/// Returns the arguments to prepend to the command execution. The caller
/// is responsible for spawning the sandbox binary with these arguments.
///
/// TS: getSandboxArgs() pattern from sandbox-adapter.ts
pub fn get_sandbox_args(config: &SandboxConfig, platform: Platform, cwd: &Path) -> Vec<String> {
    match platform {
        Platform::Linux => get_linux_sandbox_args(config, cwd),
        Platform::MacOs => get_macos_sandbox_args(config, cwd),
    }
}

/// Generate bubblewrap (bwrap) arguments for Linux sandboxing.
fn get_linux_sandbox_args(config: &SandboxConfig, cwd: &Path) -> Vec<String> {
    let binary = config.platform_binary.as_deref().unwrap_or("bwrap");

    let mut args = vec![binary.to_string()];

    // Basic filesystem: read-only bind of root
    args.extend_from_slice(&["--ro-bind".into(), "/".into(), "/".into()]);

    // Writable roots
    for root in &config.writable_roots {
        let root_str = root.display().to_string();
        args.extend_from_slice(&["--bind".into(), root_str.clone(), root_str]);
    }

    // Working directory is always writable
    let cwd_str = cwd.display().to_string();
    args.extend_from_slice(&["--bind".into(), cwd_str.clone(), cwd_str]);

    // /tmp and /dev/null access
    args.extend_from_slice(&[
        "--tmpfs".into(),
        "/tmp".into(),
        "--dev".into(),
        "/dev".into(),
    ]);

    // /proc is needed for many tools
    args.extend_from_slice(&["--proc".into(), "/proc".into()]);

    // Network isolation
    if !config.allow_network {
        args.push("--unshare-net".into());
    }

    // Disable new privileges
    args.push("--new-session".into());
    args.push("--die-with-parent".into());

    args
}

/// Generate Seatbelt (sandbox-exec) arguments for macOS sandboxing.
fn get_macos_sandbox_args(config: &SandboxConfig, cwd: &Path) -> Vec<String> {
    let binary = config.platform_binary.as_deref().unwrap_or("sandbox-exec");

    let mut profile_parts = vec!["(version 1)".to_string()];

    // Default deny
    profile_parts.push("(deny default)".into());

    // Allow read access to everything
    profile_parts.push("(allow file-read*)".into());

    // Allow process execution
    profile_parts.push("(allow process-exec)".into());
    profile_parts.push("(allow process-fork)".into());

    // Write access to CWD
    let cwd_str = cwd.display().to_string();
    profile_parts.push(format!("(allow file-write* (subpath \"{cwd_str}\"))"));

    // Write access to writable roots
    for root in &config.writable_roots {
        let root_str = root.display().to_string();
        profile_parts.push(format!("(allow file-write* (subpath \"{root_str}\"))"));
    }

    // Write access to /tmp
    profile_parts.push("(allow file-write* (subpath \"/tmp\"))".into());
    profile_parts.push("(allow file-write* (subpath \"/private/tmp\"))".into());

    // Network
    if config.allow_network {
        profile_parts.push("(allow network*)".into());
    }

    // Sysctl for basic operation
    profile_parts.push("(allow sysctl-read)".into());

    let profile = profile_parts.join("\n");

    vec![binary.to_string(), "-p".into(), profile]
}

#[cfg(test)]
#[path = "sandbox.test.rs"]
mod tests;
