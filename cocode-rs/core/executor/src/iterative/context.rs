//! Loop context for cross-iteration state passing.
//!
//! Stores iteration state for enhanced prompt injection and git tracking.

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

/// Loop execution context.
///
/// Stores cross-iteration state for building enhanced prompts and persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationContext {
    /// Current iteration number (0-based).
    pub iteration: i32,

    /// Total number of planned iterations (may be approximate for
    /// duration/until conditions, -1 for unknown).
    pub total_iterations: i32,

    /// Base commit ID at task start.
    #[serde(default)]
    pub base_commit_id: Option<String>,

    /// Original user prompt.
    #[serde(default)]
    pub initial_prompt: String,

    /// Plan file content (if exists).
    #[serde(default)]
    pub plan_content: Option<String>,

    /// Completed iteration records.
    #[serde(default)]
    pub iterations: Vec<IterationRecord>,
}

impl IterationContext {
    /// Create new IterationContext with basic info.
    pub fn new(iteration: i32, total_iterations: i32) -> Self {
        Self {
            iteration,
            total_iterations,
            base_commit_id: None,
            initial_prompt: String::new(),
            plan_content: None,
            iterations: Vec::new(),
        }
    }

    /// Create new IterationContext with full context passing enabled.
    pub fn with_context_passing(
        base_commit_id: String,
        initial_prompt: String,
        plan_content: Option<String>,
        total_iterations: i32,
    ) -> Self {
        Self {
            iteration: 0,
            total_iterations,
            base_commit_id: Some(base_commit_id),
            initial_prompt,
            plan_content,
            iterations: Vec::new(),
        }
    }

    /// Add iteration record.
    pub fn add_iteration(&mut self, record: IterationRecord) {
        self.iterations.push(record);
    }

    /// Get current iteration (next to execute).
    pub fn current_iteration(&self) -> i32 {
        self.iterations.len() as i32
    }

    /// Get successful iteration count.
    pub fn successful_iterations(&self) -> i32 {
        self.iterations.iter().filter(|r| r.success).count() as i32
    }

    /// Get failed iteration count.
    pub fn failed_iterations(&self) -> i32 {
        self.iterations.iter().filter(|r| !r.success).count() as i32
    }

    /// Get results from all previous iterations (for backward compat).
    pub fn previous_results(&self) -> Vec<String> {
        self.iterations.iter().map(|r| r.result.clone()).collect()
    }

    /// Check if context passing is enabled.
    pub fn context_passing_enabled(&self) -> bool {
        self.base_commit_id.is_some()
    }
}

/// Record of a single completed iteration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationRecord {
    /// Iteration number (0-based).
    pub iteration: i32,

    /// The result text produced by this iteration.
    pub result: String,

    /// Wall-clock duration of this iteration in milliseconds.
    pub duration_ms: i64,

    /// Commit ID (None if no changes).
    #[serde(default)]
    pub commit_id: Option<String>,

    /// Changed files list.
    #[serde(default)]
    pub changed_files: Vec<String>,

    /// LLM-generated or file-based summary.
    #[serde(default)]
    pub summary: String,

    /// Whether iteration succeeded.
    #[serde(default = "default_success")]
    pub success: bool,

    /// Completion timestamp.
    #[serde(default = "Utc::now")]
    pub timestamp: DateTime<Utc>,
}

fn default_success() -> bool {
    true
}

impl IterationRecord {
    /// Create a basic iteration record (backward compat).
    pub fn new(iteration: i32, result: String, duration_ms: i64) -> Self {
        Self {
            iteration,
            result,
            duration_ms,
            commit_id: None,
            changed_files: Vec::new(),
            summary: String::new(),
            success: true,
            timestamp: Utc::now(),
        }
    }

    /// Create a full iteration record with git info.
    pub fn with_git_info(
        iteration: i32,
        result: String,
        duration_ms: i64,
        commit_id: Option<String>,
        changed_files: Vec<String>,
        summary: String,
        success: bool,
    ) -> Self {
        Self {
            iteration,
            result,
            duration_ms,
            commit_id,
            changed_files,
            summary,
            success,
            timestamp: Utc::now(),
        }
    }

    /// Format commit status for display.
    pub fn commit_status(&self) -> String {
        match &self.commit_id {
            Some(id) if id.len() >= 7 => format!("commit {}", &id[..7]),
            Some(id) => format!("commit {id}"),
            None => "no changes".to_string(),
        }
    }

    /// Format file list for display.
    pub fn files_display(&self) -> String {
        if self.changed_files.is_empty() {
            "none".to_string()
        } else if self.changed_files.len() <= 5 {
            self.changed_files.join(", ")
        } else {
            format!(
                "{}, ... ({} more)",
                self.changed_files[..5].join(", "),
                self.changed_files.len() - 5
            )
        }
    }
}

#[cfg(test)]
#[path = "context.test.rs"]
mod tests;
