use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// Result of a shell command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    /// New CWD if command changed directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_cwd: Option<PathBuf>,
    /// Whether the command timed out.
    #[serde(default)]
    pub timed_out: bool,
}

/// Options for shell command execution.
#[derive(Debug, Clone)]
pub struct ExecOptions {
    /// Timeout in milliseconds.
    pub timeout_ms: Option<i64>,
    /// Prevent CWD changes.
    pub prevent_cwd_changes: bool,
    /// Whether to use sandbox.
    pub should_use_sandbox: bool,
    /// Extra environment variables.
    pub extra_env: HashMap<String, String>,
    /// CWD override.
    pub cwd_override: Option<PathBuf>,
}

impl Default for ExecOptions {
    fn default() -> Self {
        Self {
            timeout_ms: Some(120_000), // 2 minutes default
            prevent_cwd_changes: false,
            should_use_sandbox: false,
            extra_env: HashMap::new(),
            cwd_override: None,
        }
    }
}
