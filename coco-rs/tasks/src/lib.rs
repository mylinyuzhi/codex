//! Background task system (LocalShell, LocalAgent, Workflow).
//!
//! TS: tasks/ + Task.ts (TaskType, TaskStatus, TaskStateBase, TaskHandle)

pub mod output;
pub mod todo;

use coco_types::TaskStateBase;
use coco_types::TaskStatus;
use coco_types::TaskType;
use coco_types::generate_task_id;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Output captured from a completed task.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Task manager — tracks background tasks and their outputs.
pub struct TaskManager {
    tasks: Arc<RwLock<HashMap<String, TaskStateBase>>>,
    outputs: Arc<RwLock<HashMap<String, TaskOutput>>>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            outputs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new task.
    pub async fn create(
        &self,
        task_type: TaskType,
        description: &str,
        output_file: &str,
    ) -> String {
        let id = generate_task_id(task_type);
        let state = TaskStateBase {
            id: id.clone(),
            task_type,
            status: TaskStatus::Pending,
            description: description.to_string(),
            tool_use_id: None,
            start_time: current_time_ms(),
            end_time: None,
            total_paused_ms: None,
            output_file: output_file.to_string(),
            output_offset: 0,
            notified: false,
        };
        self.tasks.write().await.insert(id.clone(), state);
        id
    }

    /// Get a task by ID.
    pub async fn get(&self, id: &str) -> Option<TaskStateBase> {
        self.tasks.read().await.get(id).cloned()
    }

    /// Update task status.
    pub async fn update_status(&self, id: &str, status: TaskStatus) {
        if let Some(task) = self.tasks.write().await.get_mut(id) {
            task.status = status;
            if status.is_terminal() {
                task.end_time = Some(current_time_ms());
            }
        }
    }

    /// Stop a task by setting its status to Cancelled.
    pub async fn stop(&self, id: &str) {
        self.update_status(id, TaskStatus::Cancelled).await;
    }

    /// Store output for a task.
    pub async fn set_output(&self, id: &str, output: TaskOutput) {
        self.outputs.write().await.insert(id.to_string(), output);
    }

    /// Retrieve stored output for a task.
    pub async fn get_output(&self, id: &str) -> Option<TaskOutput> {
        self.outputs.read().await.get(id).cloned()
    }

    /// List all tasks.
    pub async fn list(&self) -> Vec<TaskStateBase> {
        self.tasks.read().await.values().cloned().collect()
    }

    /// Remove all tasks in a terminal state (Completed, Failed, Killed, Cancelled)
    /// and their associated outputs. Returns the number of tasks removed.
    pub async fn remove_completed(&self) -> usize {
        let mut tasks = self.tasks.write().await;
        let mut outputs = self.outputs.write().await;

        let terminal_ids: Vec<String> = tasks
            .iter()
            .filter(|(_, t)| t.status.is_terminal())
            .map(|(id, _)| id.clone())
            .collect();

        let count = terminal_ids.len();
        for id in &terminal_ids {
            tasks.remove(id);
            outputs.remove(id);
        }
        count
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Task dependency tracking.
///
/// TS: TodoV2 types — blocks/blockedBy relationships.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskDependencies {
    /// Task IDs that this task blocks (can't start until this completes).
    #[serde(default)]
    pub blocks: Vec<String>,
    /// Task IDs that must complete before this task can start.
    #[serde(default)]
    pub blocked_by: Vec<String>,
}

/// Extended task manager with persistence and dependency tracking.
pub struct PersistentTaskManager {
    inner: TaskManager,
    deps: Arc<RwLock<HashMap<String, TaskDependencies>>>,
    persist_path: Option<std::path::PathBuf>,
}

impl PersistentTaskManager {
    /// Create a new persistent task manager.
    /// If `persist_path` is Some, tasks are saved/loaded from disk as JSON.
    pub fn new(persist_path: Option<std::path::PathBuf>) -> Self {
        Self {
            inner: TaskManager::new(),
            deps: Arc::new(RwLock::new(HashMap::new())),
            persist_path,
        }
    }

    /// Get the inner task manager.
    pub fn inner(&self) -> &TaskManager {
        &self.inner
    }

    /// Add dependency: task_id blocks blocked_id.
    pub async fn add_blocks(&self, task_id: &str, blocked_id: &str) {
        let mut deps = self.deps.write().await;
        deps.entry(task_id.to_string())
            .or_default()
            .blocks
            .push(blocked_id.to_string());
        deps.entry(blocked_id.to_string())
            .or_default()
            .blocked_by
            .push(task_id.to_string());
    }

    /// Get dependencies for a task.
    pub async fn get_deps(&self, task_id: &str) -> TaskDependencies {
        self.deps
            .read()
            .await
            .get(task_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Check if a task is blocked (has unfinished blockers).
    pub async fn is_blocked(&self, task_id: &str) -> bool {
        let deps = self.deps.read().await;
        let tasks = self.inner.tasks.read().await;

        if let Some(d) = deps.get(task_id) {
            d.blocked_by.iter().any(|blocker_id| {
                tasks
                    .get(blocker_id)
                    .is_some_and(|t| !t.status.is_terminal())
            })
        } else {
            false
        }
    }

    /// Save task state to disk (if persist_path is set).
    pub async fn save(&self) -> anyhow::Result<()> {
        let Some(ref path) = self.persist_path else {
            return Ok(());
        };

        let tasks = self.inner.tasks.read().await;
        let deps = self.deps.read().await;

        let data = serde_json::json!({
            "tasks": *tasks,
            "dependencies": *deps,
        });

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(&data)?)?;
        Ok(())
    }

    /// Load task state from disk (if persist_path is set and file exists).
    pub async fn load(&self) -> anyhow::Result<()> {
        let Some(ref path) = self.persist_path else {
            return Ok(());
        };

        if !path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(path)?;
        let data: serde_json::Value = serde_json::from_str(&content)?;

        if let Some(tasks) = data.get("tasks") {
            let loaded: HashMap<String, TaskStateBase> = serde_json::from_value(tasks.clone())?;
            *self.inner.tasks.write().await = loaded;
        }

        if let Some(deps) = data.get("dependencies") {
            let loaded: HashMap<String, TaskDependencies> = serde_json::from_value(deps.clone())?;
            *self.deps.write().await = loaded;
        }

        Ok(())
    }
}

fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
