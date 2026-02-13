//! Plan file management utilities.
//!
//! Provides functions for managing plan files at `~/.cocode/plans/`.

use std::path::Path;
use std::path::PathBuf;

use snafu::ResultExt;

use crate::error::Result;
use crate::error::plan_mode_error;
use crate::plan_slug::get_unique_slug;

/// Default plan directory name within the cocode config directory.
const PLAN_DIR_NAME: &str = "plans";

/// Resolve the cocode home directory.
///
/// Checks `COCODE_HOME` env var first, falls back to `~/.cocode`.
/// This is a standalone implementation to avoid depending on `cocode-config`.
fn find_cocode_home() -> PathBuf {
    std::env::var("COCODE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".cocode")
        })
}

/// Get the plan directory path (`~/.cocode/plans/`).
pub fn get_plan_dir() -> PathBuf {
    find_cocode_home().join(PLAN_DIR_NAME)
}

/// Get the plan file path for a session.
///
/// # Arguments
///
/// * `session_id` - The session identifier for slug generation
/// * `agent_id` - Optional agent ID for subagent plans
///
/// # Returns
///
/// Path to the plan file. For subagents, the format is `{slug}-agent-{agent_id}.md`.
pub fn get_plan_file_path(session_id: &str, agent_id: Option<&str>) -> PathBuf {
    let plan_dir = get_plan_dir();
    let slug = get_unique_slug(session_id, None);

    let filename = match agent_id {
        Some(id) => format!("{slug}-agent-{id}.md"),
        None => format!("{slug}.md"),
    };

    plan_dir.join(filename)
}

/// Read the contents of a plan file.
///
/// # Arguments
///
/// * `session_id` - The session identifier
/// * `agent_id` - Optional agent ID for subagent plans
///
/// # Returns
///
/// `Some(content)` if the file exists and is readable, `None` if it doesn't exist.
pub fn read_plan_file(session_id: &str, agent_id: Option<&str>) -> Option<String> {
    let path = get_plan_file_path(session_id, agent_id);
    std::fs::read_to_string(&path).ok()
}

/// Check if a path is a plan file (for permission exceptions).
///
/// # Arguments
///
/// * `path` - The path to check
/// * `plan_path` - The expected plan file path
///
/// # Returns
///
/// `true` if the paths match (allowing Write/Edit tool usage in plan mode).
pub fn is_plan_file(path: &Path, plan_path: &Path) -> bool {
    // Normalize paths for comparison
    path == plan_path
}

/// Ensure the plan directory exists.
///
/// # Errors
///
/// Returns an error if directory creation fails.
pub fn ensure_plan_dir() -> Result<PathBuf> {
    let plan_dir = get_plan_dir();
    if !plan_dir.exists() {
        std::fs::create_dir_all(&plan_dir).context(plan_mode_error::CreateDirSnafu {
            message: format!("failed to create {}", plan_dir.display()),
        })?;
    }
    Ok(plan_dir)
}

/// Manager for plan file operations.
///
/// Provides a higher-level API for plan file management with session context.
#[derive(Debug, Clone)]
pub struct PlanFileManager {
    session_id: String,
    agent_id: Option<String>,
}

impl PlanFileManager {
    /// Create a new plan file manager.
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            agent_id: None,
        }
    }

    /// Create a new plan file manager for a subagent.
    pub fn for_agent(session_id: impl Into<String>, agent_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            agent_id: Some(agent_id.into()),
        }
    }

    /// Get the plan file path.
    pub fn path(&self) -> PathBuf {
        get_plan_file_path(&self.session_id, self.agent_id.as_deref())
    }

    /// Ensure the plan directory exists and return the plan file path.
    pub fn ensure_and_get_path(&self) -> Result<PathBuf> {
        ensure_plan_dir()?;
        Ok(self.path())
    }

    /// Read the plan file contents.
    pub fn read(&self) -> Option<String> {
        read_plan_file(&self.session_id, self.agent_id.as_deref())
    }

    /// Check if a path matches this manager's plan file.
    pub fn is_plan_file(&self, path: &Path) -> bool {
        is_plan_file(path, &self.path())
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the agent ID if this is a subagent manager.
    pub fn agent_id(&self) -> Option<&str> {
        self.agent_id.as_deref()
    }
}

#[cfg(test)]
#[path = "plan_file.test.rs"]
mod tests;
