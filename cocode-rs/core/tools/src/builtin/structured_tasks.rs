//! Shared state for structured task management tools.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Status of a structured task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Completed => "completed",
            TaskStatus::Deleted => "deleted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(TaskStatus::Pending),
            "in_progress" => Some(TaskStatus::InProgress),
            "completed" => Some(TaskStatus::Completed),
            "deleted" => Some(TaskStatus::Deleted),
            _ => None,
        }
    }

    /// Returns `true` if the transition from `self` to `to` is valid.
    ///
    /// Valid transitions:
    /// - `Pending → InProgress`, `Pending → Completed`
    /// - `InProgress → Completed`
    /// - Any status → `Deleted`
    pub fn can_transition_to(self, to: TaskStatus) -> bool {
        if to == TaskStatus::Deleted {
            return true;
        }
        matches!(
            (self, to),
            (TaskStatus::Pending, TaskStatus::InProgress)
                | (TaskStatus::Pending, TaskStatus::Completed)
                | (TaskStatus::InProgress, TaskStatus::Completed)
        )
    }
}

/// A structured task with dependencies and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredTask {
    pub id: String,
    pub subject: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,
    /// Optional owner (agent/team name) for task assignment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// IDs of tasks this task blocks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<String>,
    /// IDs of tasks that block this task.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<String>,
    /// Arbitrary metadata.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub metadata: Value,
}

/// Shared state for structured tasks, accessible across tools.
pub type StructuredTaskStore = Arc<Mutex<BTreeMap<String, StructuredTask>>>;

/// Create a new empty task store.
pub fn new_task_store() -> StructuredTaskStore {
    Arc::new(Mutex::new(BTreeMap::new()))
}

/// Generate a short unique task ID.
pub fn generate_task_id() -> String {
    let uuid = uuid::Uuid::new_v4();
    // Use first 8 chars of UUID for short IDs
    format!("task_{}", &uuid.to_string()[..8])
}

/// Serialize the full task store to a JSON value for ContextModifier.
pub fn tasks_to_value(tasks: &BTreeMap<String, StructuredTask>) -> Value {
    serde_json::to_value(tasks).unwrap_or_else(|e| {
        tracing::error!("StructuredTask serialization failed: {e}");
        Value::Object(Default::default())
    })
}

/// Returns `true` if the task is marked as internal via `metadata._internal`.
pub fn is_internal_task(task: &StructuredTask) -> bool {
    task.metadata
        .get("_internal")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Format a single task as a human-readable line.
///
/// When `completed_ids` is provided, `blocked_by` entries that appear in the set
/// are filtered out (smart blocker filtering — only show incomplete blockers).
///
/// Returns `None` for deleted tasks and internal tasks (`metadata._internal == true`).
pub fn format_single_task(
    task: &StructuredTask,
    completed_ids: Option<&HashSet<String>>,
) -> Option<String> {
    if matches!(task.status, TaskStatus::Deleted) || is_internal_task(task) {
        return None;
    }
    let marker = match task.status {
        TaskStatus::Completed => "[x]",
        TaskStatus::InProgress => "[>]",
        TaskStatus::Pending => "[ ]",
        TaskStatus::Deleted => return None,
    };
    let mut out = format!("{marker} {}: {}\n", task.id, task.subject);

    // blocked_by — optionally filter out completed blockers
    let active_blockers: Vec<&str> = task
        .blocked_by
        .iter()
        .filter(|b| completed_ids.is_none_or(|ids| !ids.contains(b.as_str())))
        .map(String::as_str)
        .collect();
    if !active_blockers.is_empty() {
        out.push_str(&format!("    blocked by: {}\n", active_blockers.join(", ")));
    }
    if let Some(ref owner) = task.owner {
        out.push_str(&format!("    owner: {owner}\n"));
    }
    if !task.blocks.is_empty() {
        out.push_str(&format!("    blocks: {}\n", task.blocks.join(", ")));
    }
    Some(out)
}

/// Format tasks as a human-readable summary.
pub fn format_task_summary(tasks: &BTreeMap<String, StructuredTask>) -> String {
    format_task_summary_filtered(tasks, None)
}

/// Format tasks as a human-readable summary with optional smart blocker filtering.
pub fn format_task_summary_filtered(
    tasks: &BTreeMap<String, StructuredTask>,
    completed_ids: Option<&HashSet<String>>,
) -> String {
    if tasks.is_empty() {
        return "No tasks.".to_string();
    }
    let mut output = String::new();
    for task in tasks.values() {
        if let Some(line) = format_single_task(task, completed_ids) {
            output.push_str(&line);
        }
    }
    if output.is_empty() {
        return "No tasks.".to_string();
    }
    output
}

/// Derive present-continuous `active_form` from a task subject.
///
/// Converts imperative form (e.g., "Fix auth bug") to present continuous
/// (e.g., "Fixing auth bug"). Falls back to "Working on: {subject}" for
/// unrecognized verbs.
pub fn derive_active_form(subject: &str) -> String {
    let trimmed = subject.trim();
    if trimmed.is_empty() {
        return "Working on task".to_string();
    }

    // Common verb mappings: imperative → present continuous
    const VERB_MAP: &[(&str, &str)] = &[
        ("fix", "Fixing"),
        ("add", "Adding"),
        ("update", "Updating"),
        ("remove", "Removing"),
        ("create", "Creating"),
        ("implement", "Implementing"),
        ("refactor", "Refactoring"),
        ("delete", "Deleting"),
        ("write", "Writing"),
        ("build", "Building"),
        ("set", "Setting"),
        ("run", "Running"),
        ("test", "Testing"),
        ("check", "Checking"),
        ("move", "Moving"),
        ("merge", "Merging"),
        ("configure", "Configuring"),
        ("deploy", "Deploying"),
        ("install", "Installing"),
        ("debug", "Debugging"),
    ];

    let lower = trimmed.to_lowercase();
    for (verb, continuous) in VERB_MAP {
        if let Some(rest) = lower.strip_prefix(verb)
            && (rest.is_empty() || rest.starts_with(' '))
        {
            let original_rest = &trimmed[verb.len()..];
            return format!("{continuous}{original_rest}");
        }
    }

    format!("Working on: {trimmed}")
}

#[cfg(test)]
#[path = "structured_tasks.test.rs"]
mod tests;
