//! Command input and result types for shell execution.

use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// Extracted file paths from command output.
///
/// When a fast model is configured, the shell executor can analyze command
/// output to extract file paths that the command read or modified. This enables
/// fast model pre-reading for improved context.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ExtractedPaths {
    /// File paths extracted from command output.
    pub paths: Vec<PathBuf>,
    /// Whether extraction was attempted.
    pub extraction_attempted: bool,
    /// Duration of extraction in milliseconds.
    pub extraction_ms: i64,
}

impl ExtractedPaths {
    /// Creates a new ExtractedPaths with the given paths.
    pub fn new(paths: Vec<PathBuf>, extraction_ms: i64) -> Self {
        Self {
            paths,
            extraction_attempted: true,
            extraction_ms,
        }
    }

    /// Creates an ExtractedPaths indicating extraction was not attempted.
    pub fn not_attempted() -> Self {
        Self {
            paths: Vec::new(),
            extraction_attempted: false,
            extraction_ms: 0,
        }
    }

    /// Returns true if any paths were extracted.
    pub fn has_paths(&self) -> bool {
        !self.paths.is_empty()
    }
}

/// Result of a shell command execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandResult {
    /// Process exit code (0 = success).
    pub exit_code: i32,
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
    /// Execution duration in milliseconds.
    pub duration_ms: i64,
    /// Whether the output was truncated due to size limits.
    pub truncated: bool,
    /// New working directory after command execution (if changed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_cwd: Option<PathBuf>,
    /// File paths extracted from command output (when fast model configured).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extracted_paths: Option<ExtractedPaths>,
}

impl CommandResult {
    /// Returns true if the command exited successfully (exit code 0).
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Input parameters for a shell command execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandInput {
    /// The shell command to execute.
    pub command: String,
    /// Optional timeout in milliseconds. Defaults to executor's default if None.
    #[serde(default)]
    pub timeout_ms: Option<i64>,
    /// Optional working directory override.
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    /// Optional human-readable description of what the command does.
    #[serde(default)]
    pub description: Option<String>,
    /// Whether to run the command in the background.
    #[serde(default)]
    pub run_in_background: bool,
}

#[cfg(test)]
#[path = "command.test.rs"]
mod tests;
