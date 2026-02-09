//! Sandbox configuration types.

use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// Sandbox execution mode controlling filesystem and network access.
///
/// This is the sandbox crate's own mode enum, distinct from `cocode_protocol::SandboxMode`
/// which is focused on protocol-level configuration. This enum maps the protocol mode
/// into enforcement categories.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxMode {
    /// No sandbox enforcement; all operations are allowed.
    #[default]
    None,
    /// Read-only mode; file writes are blocked.
    ReadOnly,
    /// Strict mode; only explicitly allowed paths are accessible,
    /// and network is blocked unless explicitly allowed.
    Strict,
}

/// Configuration for the sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// The sandbox enforcement mode.
    #[serde(default)]
    pub mode: SandboxMode,
    /// Paths that are explicitly allowed for read/write access.
    #[serde(default)]
    pub allowed_paths: Vec<PathBuf>,
    /// Paths that are explicitly denied (takes precedence over allowed).
    #[serde(default)]
    pub denied_paths: Vec<PathBuf>,
    /// Whether network access is allowed.
    #[serde(default)]
    pub allow_network: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            mode: SandboxMode::default(),
            allowed_paths: Vec::new(),
            denied_paths: Vec::new(),
            allow_network: false,
        }
    }
}

/// User/policy-level sandbox settings.
///
/// These settings control whether sandboxing is enabled and how bypass requests
/// are handled. Based on Claude Code's architecture where sandbox is **optional
/// and disabled by default**.
///
/// # Default Behavior
///
/// By default, sandbox is disabled (`enabled: false`), which means:
/// - Commands execute directly without any sandbox wrapping
/// - No Landlock/Seatbelt enforcement is applied
/// - `is_sandboxed()` returns `false`
///
/// This matches Claude Code's behavior where non-sandbox mode is the default
/// execution path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxSettings {
    /// Enable sandbox mode.
    ///
    /// When `false` (default), commands run directly without sandbox wrapping.
    /// When `true`, commands are wrapped with platform-specific sandbox
    /// (Landlock on Linux, Seatbelt on macOS).
    #[serde(default)]
    pub enabled: bool,

    /// Auto-approve bash commands when running in sandbox mode.
    ///
    /// When `true` (default), bash commands that would normally require
    /// approval can run automatically if the sandbox is enabled.
    #[serde(default = "default_true")]
    pub auto_allow_bash_if_sandboxed: bool,

    /// Allow the `dangerously_disable_sandbox` parameter to bypass sandbox.
    ///
    /// When `true` (default), individual commands can request sandbox bypass
    /// using the `dangerously_disable_sandbox` flag.
    #[serde(default = "default_true")]
    pub allow_unsandboxed_commands: bool,
}

fn default_true() -> bool {
    true
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self {
            enabled: false, // Sandbox disabled by default
            auto_allow_bash_if_sandboxed: true,
            allow_unsandboxed_commands: true,
        }
    }
}

impl SandboxSettings {
    /// Creates settings with sandbox enabled.
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Default::default()
        }
    }

    /// Creates settings with sandbox disabled (same as default).
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Check if a command should run in sandbox mode.
    ///
    /// Returns `false` (no sandbox) if:
    /// 1. Sandbox is disabled (`!self.enabled`)
    /// 2. Bypass requested and allowed (`dangerously_disable_sandbox && allow_unsandboxed_commands`)
    /// 3. Command is empty
    ///
    /// # Arguments
    ///
    /// * `command` - The shell command to check
    /// * `dangerously_disable_sandbox` - Whether bypass was requested for this command
    pub fn is_sandboxed(&self, command: &str, dangerously_disable_sandbox: bool) -> bool {
        // 1. Sandbox disabled → no sandbox
        if !self.enabled {
            return false;
        }

        // 2. Bypass requested and allowed
        if dangerously_disable_sandbox && self.allow_unsandboxed_commands {
            return false;
        }

        // 3. Empty command
        if command.trim().is_empty() {
            return false;
        }

        // Otherwise: sandbox if enabled
        true
    }
}

#[cfg(test)]
#[path = "config.test.rs"]
mod tests;
