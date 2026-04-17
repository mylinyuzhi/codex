//! TodoV2 task management — the in-conversation task list.
//!
//! TS: utils/todo/ + Task.ts TodoItem type

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex;

/// TodoV2 item status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

impl TodoStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Deleted)
    }
}

/// A TodoV2 item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub subject: String,
    #[serde(default)]
    pub description: String,
    pub status: TodoStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default)]
    pub blocks: Vec<String>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
}

/// Global todo store (in-memory, per-session).
static TODO_STORE: LazyLock<Mutex<Vec<TodoItem>>> = LazyLock::new(|| Mutex::new(Vec::new()));

static NEXT_ID: LazyLock<Mutex<i32>> = LazyLock::new(|| Mutex::new(1));

fn generate_id() -> String {
    let mut id = NEXT_ID
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let current = *id;
    *id += 1;
    current.to_string()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Create a new todo item.
pub fn create_todo(subject: &str, description: &str, active_form: Option<&str>) -> TodoItem {
    let item = TodoItem {
        id: generate_id(),
        subject: subject.to_string(),
        description: description.to_string(),
        status: TodoStatus::Pending,
        active_form: active_form.map(String::from),
        owner: None,
        blocks: Vec::new(),
        blocked_by: Vec::new(),
        metadata: None,
        created_at: now_ms(),
        completed_at: None,
    };
    TODO_STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(item.clone());
    item
}

/// Get a todo by ID.
pub fn get_todo(id: &str) -> Option<TodoItem> {
    TODO_STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .find(|t| t.id == id)
        .cloned()
}

/// Update a todo's status.
pub fn update_todo_status(id: &str, status: TodoStatus) -> Option<TodoItem> {
    let mut store = TODO_STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(item) = store.iter_mut().find(|t| t.id == id) {
        item.status = status;
        if status.is_terminal() {
            item.completed_at = Some(now_ms());
        }
        Some(item.clone())
    } else {
        None
    }
}

/// List all todos.
pub fn list_todos() -> Vec<TodoItem> {
    TODO_STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// List active (non-terminal) todos.
pub fn list_active_todos() -> Vec<TodoItem> {
    TODO_STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter(|t| !t.status.is_terminal())
        .cloned()
        .collect()
}

/// Add a blocks relationship.
pub fn add_blocks(blocker_id: &str, blocked_id: &str) {
    let mut store = TODO_STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(blocker) = store.iter_mut().find(|t| t.id == blocker_id)
        && !blocker.blocks.contains(&blocked_id.to_string())
    {
        blocker.blocks.push(blocked_id.to_string());
    }
    if let Some(blocked) = store.iter_mut().find(|t| t.id == blocked_id)
        && !blocked.blocked_by.contains(&blocker_id.to_string())
    {
        blocked.blocked_by.push(blocker_id.to_string());
    }
}

/// Clear all todos (for testing).
pub fn clear_todos() {
    TODO_STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    *NEXT_ID
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = 1;
}

/// Format todos as markdown for display.
pub fn format_todos_markdown(todos: &[TodoItem]) -> String {
    if todos.is_empty() {
        return "No tasks.".to_string();
    }
    todos
        .iter()
        .map(|t| {
            let icon = match t.status {
                TodoStatus::Pending => "○",
                TodoStatus::InProgress => "◑",
                TodoStatus::Completed => "●",
                TodoStatus::Deleted => "✕",
            };
            let blocked = if t.blocked_by.is_empty() {
                String::new()
            } else {
                format!(" (blocked by: {})", t.blocked_by.join(", "))
            };
            format!(
                "{icon} #{} [{}] {}{}\n  {}",
                t.id,
                match t.status {
                    TodoStatus::Pending => "pending",
                    TodoStatus::InProgress => "in_progress",
                    TodoStatus::Completed => "completed",
                    TodoStatus::Deleted => "deleted",
                },
                t.subject,
                blocked,
                t.description
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[path = "todo.test.rs"]
mod tests;
