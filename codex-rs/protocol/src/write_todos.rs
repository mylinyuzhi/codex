use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

// Types for the write-todos tool, inspired by gemini-cli's implementation
// This tool helps track execution progress with fine-grained task management

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    /// Work has not begun on this task
    Pending,
    /// Currently working on this task (only one task can be in_progress at a time)
    InProgress,
    /// Task has been successfully completed
    Completed,
    /// Task is no longer needed and has been cancelled
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct TodoItem {
    /// Description of the task to be done
    pub description: String,
    /// Current status of the task
    pub status: TodoStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct WriteTodosArgs {
    /// Complete list of todos (replaces existing list)
    pub todos: Vec<TodoItem>,
}
