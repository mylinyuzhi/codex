//! Shared DTOs for the persistent task list (V2) and per-agent todos
//! (V1). Lives in `coco-types` so [`crate::app_state::ToolAppState`]
//! can carry typed snapshots without depending on the higher-level
//! handle/implementation crates (`coco-tool`, `coco-tasks`).
//!
//! TS parity: `utils/tasks.ts` (`TaskSchema`, `TaskStatusSchema`) +
//! `utils/todo/types.ts` (`TodoItemSchema`).

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

// ── Task (V2) DTOs ────────────────────────────────────────────────────

/// Task status wire format — matches TS `TaskStatusSchema`
/// (`utils/tasks.ts:69-74`). **Distinct** from [`crate::TaskStatus`],
/// which is the 6-variant running-task lifecycle enum.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskListStatus {
    Pending,
    InProgress,
    Completed,
}

impl TaskListStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }
}

/// A durable plan-item, matching TS `TaskSchema` (`utils/tasks.ts:76-89`).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub subject: String,
    #[serde(default)]
    pub description: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "activeForm"
    )]
    pub active_form: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub status: TaskListStatus,
    #[serde(default)]
    pub blocks: Vec<String>,
    #[serde(default, rename = "blockedBy")]
    pub blocked_by: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// Partial update passed to a task-list handle's `update_task`.
#[derive(Debug, Clone, Default)]
pub struct TaskRecordUpdate {
    pub subject: Option<String>,
    pub description: Option<String>,
    pub active_form: Option<String>,
    pub owner: Option<String>,
    pub status: Option<TaskListStatus>,
    /// Merge these keys into `metadata`; `null` values delete a key.
    pub metadata_merge: Option<HashMap<String, serde_json::Value>>,
}

/// Outcome of a `claim_task` call (TS `ClaimTaskResult`).
#[derive(Debug, Clone)]
pub enum TaskClaimOutcome {
    Success(TaskRecord),
    TaskNotFound,
    AlreadyClaimed(TaskRecord),
    AlreadyResolved(TaskRecord),
    Blocked {
        task: TaskRecord,
        blocked_by_tasks: Vec<String>,
    },
    AgentBusy {
        task: TaskRecord,
        busy_with_tasks: Vec<String>,
    },
}

// ── Todo (V1) DTOs ────────────────────────────────────────────────────

/// A TodoWrite item — byte-matches TS `TodoItemSchema` (no `id` field,
/// positional identity).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoRecord {
    pub content: String,
    pub status: String,
    #[serde(rename = "activeForm")]
    pub active_form: String,
}

// ── UI view state ─────────────────────────────────────────────────────

/// Which panel the TUI should have expanded in the task area.
/// TS parity: `AppState.expandedView` in `AppStateStore.ts` (3 variants:
/// `'none' | 'tasks' | 'teammates'`).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpandedView {
    #[default]
    None,
    Tasks,
    Teammates,
}

impl ExpandedView {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Tasks => "tasks",
            Self::Teammates => "teammates",
        }
    }
}
